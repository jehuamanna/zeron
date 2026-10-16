//! The gateway under load and faults, through real loopback connections:
//! many concurrent ranges stay byte-exact, the stream limit holds while slow
//! clients block, and clients that vanish mid-body never leak a stream slot.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test sizes are far below the limits of usize and f64"
)]

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use media_gateway::{FileResource, GatewayLimits, MediaGateway};
use mutation_testing::Rng;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct Fixture {
    root: PathBuf,
    gateway: MediaGateway,
    address: String,
    path: String,
    content: Arc<Vec<u8>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture(name: &str, bytes: usize, limits: GatewayLimits) -> Fixture {
    let root = std::env::temp_dir().join(format!("zeron-gateway-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut rng = Rng::new(0x5EED);
    let content: Vec<u8> = (0..bytes)
        .map(|_| rng.next_u64().to_le_bytes()[0])
        .collect();
    let file = root.join("lesson.bin");
    std::fs::write(&file, &content).unwrap();
    let gateway = MediaGateway::start(limits).await.unwrap();
    let registration = gateway
        .register(FileResource {
            path: file,
            media_type: "video/mp4".to_owned(),
            byte_length: bytes as u64,
        })
        .await
        .unwrap();
    let rest = registration.url.strip_prefix("http://").unwrap();
    let (address, path) = rest.split_once('/').unwrap();
    Fixture {
        root,
        gateway,
        address: address.to_owned(),
        path: format!("/{path}"),
        content: Arc::new(content),
    }
}

/// One `GET` with a `Range`, returning the status and body.
async fn get(address: &str, path: &str, range: Option<(u64, u64)>) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let range = range.map_or_else(String::new, |(start, end)| {
        format!("Range: bytes={start}-{end}\r\n")
    });
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\n{range}Connection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    if let Err(error) = stream.read_to_end(&mut response).await {
        panic!(
            "reading the response failed after {} bytes: {error}",
            response.len()
        );
    }
    let Some(split) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!(
            "no response head in {} bytes: {:?}",
            response.len(),
            String::from_utf8_lossy(&response[..response.len().min(200)])
        );
    };
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, response[split + 4..].to_vec())
}

/// Eight players at once, two per lesson: the gateway keeps the two newest
/// bodies of each resource (a newer range cancels the oldest, as after a
/// seek), so two concurrent readers per resource must never be cut short.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hundreds_of_concurrent_ranges_stay_byte_exact() {
    let limits = GatewayLimits {
        max_concurrent_streams: 8,
        ..GatewayLimits::default()
    };
    let fixture = fixture("soak", 1024 * 1024, limits).await;
    let length = fixture.content.len() as u64;
    let mut paths = vec![fixture.path.clone()];
    for _ in 0..3 {
        let registration = fixture
            .gateway
            .register(FileResource {
                path: fixture.root.join("lesson.bin"),
                media_type: "video/mp4".to_owned(),
                byte_length: length,
            })
            .await
            .unwrap();
        let rest = registration.url.strip_prefix("http://").unwrap();
        paths.push(format!("/{}", rest.split_once('/').unwrap().1));
    }
    let started = Instant::now();
    let mut workers = Vec::new();
    for worker in 0..8_u64 {
        let (address, path) = (
            fixture.address.clone(),
            paths[(worker % 4) as usize].clone(),
        );
        let content = Arc::clone(&fixture.content);
        workers.push(tokio::spawn(async move {
            let mut rng = Rng::new(worker + 1);
            let mut served = 0_u64;
            for _ in 0..50 {
                let start = rng.next_u64() % length;
                let end = (start + rng.next_u64() % (256 * 1024)).min(length - 1);
                let (status, body) = get(&address, &path, Some((start, end))).await;
                assert_eq!(status, 206, "range {start}-{end}");
                assert!(
                    body == content[start as usize..=end as usize],
                    "range {start}-{end} differs ({} bytes)",
                    body.len()
                );
                served += body.len() as u64;
            }
            served
        }));
    }
    let mut served = 0;
    for worker in workers {
        served += worker.await.unwrap();
    }
    let elapsed = started.elapsed();
    eprintln!(
        "400 ranges, {} MiB in {elapsed:?} ({:.0} MiB/s)",
        served / (1024 * 1024),
        served as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
    );
    let (status, whole) = get(&fixture.address, &fixture.path, None).await;
    assert_eq!((status, whole.len()), (200, fixture.content.len()));
    fixture.gateway.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_stream_limit_holds_and_vanished_clients_free_their_slots() {
    let limits = GatewayLimits {
        max_concurrent_streams: 2,
        ..GatewayLimits::default()
    };
    // Large enough that a client that never reads blocks its stream.
    let fixture = fixture("limit", 64 * 1024 * 1024, limits).await;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        fixture.path, fixture.address
    );
    let mut stalled = Vec::new();
    for _ in 0..2 {
        let mut stream = TcpStream::connect(&fixture.address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut first = [0_u8; 12];
        stream.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"HTTP/1.1 200");
        stalled.push(stream);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (status, _) = get(&fixture.address, &fixture.path, Some((0, 9))).await;
    assert_eq!(status, 503, "a third stream is refused while two are open");

    drop(stalled);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, body) = get(&fixture.address, &fixture.path, Some((0, 9))).await;
        if status == 206 {
            assert_eq!(body, fixture.content[..10]);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "closed clients never freed their stream slots"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Clients that read a little and vanish, many times over.
    for _ in 0..50 {
        let mut stream = TcpStream::connect(&fixture.address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut some = [0_u8; 4096];
        let _ = stream.read(&mut some).await.unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut both_free = false;
    while Instant::now() < deadline && !both_free {
        let first = get(&fixture.address, &fixture.path, Some((5, 9)));
        let second = get(&fixture.address, &fixture.path, Some((10, 19)));
        let ((a, _), (b, _)) = tokio::join!(first, second);
        both_free = a == 206 && b == 206;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(both_free, "vanished clients leaked stream slots");
    fixture.gateway.shutdown();
}

/// A longer soak for release candidates: `cargo test -p media-gateway --test
/// soak -- --ignored`. Runs for `ZERON_SOAK_SECONDS` (default 60).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "long-running soak"]
async fn sustained_random_ranges_for_a_minute() {
    let seconds = std::env::var("ZERON_SOAK_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let fixture = fixture("soak-long", 8 * 1024 * 1024, GatewayLimits::default()).await;
    let length = fixture.content.len() as u64;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rng = Rng::new(7);
    let mut requests = 0_u64;
    while Instant::now() < deadline {
        let start = rng.next_u64() % length;
        let end = (start + rng.next_u64() % (1024 * 1024)).min(length - 1);
        let (status, body) = get(&fixture.address, &fixture.path, Some((start, end))).await;
        assert_eq!(status, 206);
        assert_eq!(body, fixture.content[start as usize..=end as usize]);
        requests += 1;
    }
    eprintln!("{requests} ranges in {seconds} s");
    fixture.gateway.shutdown();
}
