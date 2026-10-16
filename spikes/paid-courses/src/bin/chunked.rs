//! Spike: cost of chunked decryption, and seeking through Zeron's real media gateway.
//!
//! `cargo run --release --bin chunked -- bench`  reader-level numbers per chunk size
//! `cargo run --release --bin chunked -- serve`  gateway + a page that times seeks in a <video>

use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use futures_util::{FutureExt, future::BoxFuture};
use media_gateway::{FileResource, GatewayLimits, MediaGateway, MediaReader, MediaSource, StreamResource};
use paid_courses_spike::{DecryptingReader, ResourceCipher, encrypt};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

const KEY: [u8; 32] = [42; 32];
const RESOURCE: &str = "lesson-2-video";

fn media_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("media")
}

/// A 3-minute 1080p H.264 file at about 8 Mbit/s with a keyframe every 2 s.
fn ensure_video() -> anyhow::Result<PathBuf> {
    let path = media_dir().join("plain.mp4");
    if !path.exists() {
        std::fs::create_dir_all(media_dir())?;
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=24:duration=180"])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=180"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-b:v", "8M", "-g", "48", "-pix_fmt", "yuv420p"])
            .args(["-c:a", "aac", "-movflags", "+faststart"])
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("ffmpeg failed");
        }
    }
    Ok(path)
}

fn encrypted(plain: &Path, chunk: usize) -> anyhow::Result<(PathBuf, ResourceCipher, Duration)> {
    let len = std::fs::metadata(plain)?.len();
    let cipher = ResourceCipher::new(&KEY, RESOURCE, chunk, len);
    let path = media_dir().join(format!("enc-{}k.bin", chunk / 1024));
    let started = Instant::now();
    if !path.exists() {
        let mut src = BufReader::new(File::open(plain)?);
        let mut dst = BufWriter::new(File::create(&path)?);
        encrypt(&mut src, &mut dst, &cipher)?;
    }
    Ok((path, cipher, started.elapsed()))
}

struct EncryptedSource {
    path: PathBuf,
    cipher: ResourceCipher,
}

impl MediaSource for EncryptedSource {
    fn open(&self) -> BoxFuture<'static, std::io::Result<Box<dyn MediaReader>>> {
        let path = self.path.clone();
        let cipher = self.cipher.clone();
        async move {
            let file = tokio::fs::File::open(path).await?;
            Ok(Box::new(DecryptingReader::new(file, cipher)) as Box<dyn MediaReader>)
        }
        .boxed()
    }
}

struct PlainSource(PathBuf);

impl MediaSource for PlainSource {
    fn open(&self) -> BoxFuture<'static, std::io::Result<Box<dyn MediaReader>>> {
        let path = self.0.clone();
        async move { Ok(Box::new(tokio::fs::File::open(path).await?) as Box<dyn MediaReader>) }.boxed()
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

async fn open_reader(plain: &Path, enc: Option<(&Path, &ResourceCipher)>) -> anyhow::Result<Box<dyn MediaReader>> {
    Ok(match enc {
        None => Box::new(tokio::fs::File::open(plain).await?),
        Some((path, cipher)) => Box::new(DecryptingReader::new(tokio::fs::File::open(path).await?, cipher.clone())),
    })
}

async fn measure(label: &str, plain: &Path, enc: Option<(&Path, &ResourceCipher)>, len: u64) -> anyhow::Result<()> {
    // Sequential: whole file in 256 KiB reads, as the gateway streams a body.
    let mut reader = open_reader(plain, enc).await?;
    let mut buf = vec![0_u8; 256 * 1024];
    let started = Instant::now();
    let mut total = 0_u64;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    let secs = started.elapsed().as_secs_f64();
    anyhow::ensure!(total == len, "short read");

    // Random seeks: a fresh reader per request (the gateway opens one per
    // response), seek, then read the first 512 KiB of the response.
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut times = Vec::new();
    let mut first_bytes = Vec::new();
    for _ in 0..300 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let at = state % (len - 600 * 1024);
        let started = Instant::now();
        let mut reader = open_reader(plain, enc).await?;
        reader.seek(SeekFrom::Start(at)).await?;
        let mut first = [0_u8; 1];
        reader.read_exact(&mut first).await?;
        first_bytes.push(started.elapsed());
        let mut rest = vec![0_u8; 512 * 1024 - 1];
        reader.read_exact(&mut rest).await?;
        times.push(started.elapsed());
    }
    times.sort();
    first_bytes.sort();
    println!(
        "{label:<14} seq {:>7.0} MB/s | first byte p50 {:>6.0?} p99 {:>6.0?} | 512 KiB p50 {:>6.0?} p99 {:>6.0?}",
        total as f64 / secs / 1e6,
        percentile(&first_bytes, 0.5),
        percentile(&first_bytes, 0.99),
        percentile(&times, 0.5),
        percentile(&times, 0.99),
    );
    Ok(())
}

async fn bench() -> anyhow::Result<()> {
    let plain = ensure_video()?;
    let len = std::fs::metadata(&plain)?.len();
    println!("video: {:.1} MB", len as f64 / 1e6);
    measure("plaintext", &plain, None, len).await?;
    for chunk in [16 * 1024, 64 * 1024, 256 * 1024, 1024 * 1024] {
        let _ = std::fs::remove_file(media_dir().join(format!("enc-{}k.bin", chunk / 1024)));
        let (path, cipher, took) = encrypted(&plain, chunk)?;
        let overhead = (cipher.cipher_len() - len) as f64 / len as f64 * 100.0;
        println!(
            "chunk {:>5} KiB: encrypt {:>5.0} MB/s, size overhead {:.3}%",
            chunk / 1024,
            len as f64 / took.as_secs_f64() / 1e6,
            overhead
        );
        measure(&format!("  decrypt {}K", chunk / 1024), &plain, Some((&path, &cipher)), len).await?;
    }
    Ok(())
}

async fn ffmpeg_frame(url: &str, at: u32) -> anyhow::Result<(String, Duration)> {
    let started = Instant::now();
    let out = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &at.to_string(), "-i", url, "-frames:v", "1", "-f", "md5", "-"])
        .output()
        .await?;
    anyhow::ensure!(out.status.success(), "ffmpeg: {}", String::from_utf8_lossy(&out.stderr));
    Ok((String::from_utf8(out.stdout)?.trim().to_owned(), started.elapsed()))
}

async fn serve() -> anyhow::Result<()> {
    let plain = ensure_video()?;
    let len = std::fs::metadata(&plain)?.len();
    let (enc_path, cipher, _) = encrypted(&plain, 64 * 1024)?;
    let gateway = MediaGateway::start(GatewayLimits::default()).await?;
    let plain_file = gateway
        .register(FileResource { path: plain.clone(), media_type: "video/mp4".into(), byte_length: len })
        .await?
        .url;
    let plain_stream = gateway
        .register_stream(StreamResource { source: Arc::new(PlainSource(plain)), media_type: "video/mp4".into(), byte_length: len })?
        .url;
    let encrypted = gateway
        .register_stream(StreamResource {
            source: Arc::new(EncryptedSource { path: enc_path, cipher }),
            media_type: "video/mp4".into(),
            byte_length: len,
        })?
        .url;

    // Correctness and timing with a real demuxer/decoder over HTTP ranges.
    for at in [0, 37, 95, 171] {
        let (a, ta) = ffmpeg_frame(&plain_stream, at).await?;
        let (b, tb) = ffmpeg_frame(&encrypted, at).await?;
        println!("ffmpeg seek {at:>3}s: plain {ta:>6.0?} encrypted {tb:>6.0?} same frame: {}", a == b);
        anyhow::ensure!(a == b, "decoded frames differ");
    }

    let page = include_str!("seek.html")
        .replace("{{PLAIN_FILE}}", &plain_file)
        .replace("{{PLAIN_STREAM}}", &plain_stream)
        .replace("{{ENCRYPTED}}", &encrypted);
    let app = axum::Router::new().route("/", axum::routing::get(move || async move { axum::response::Html(page) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("seek page: http://{}/", listener.local_addr()?);
    axum::serve(listener, app).await.context("page server")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("bench") => bench().await,
        Some("serve") => serve().await,
        _ => bail!("usage: chunked bench|serve"),
    }
}
