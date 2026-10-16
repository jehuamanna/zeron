//! The seed server as a hub, with real engines on loopback: it follows a
//! publisher's magnet, then serves learners after the publisher is gone and
//! after its own restart, and it serves copied releases.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, StructureEdit,
};
use seed_server::{
    registry::Registry,
    server::{ServeConfig, Server, serve_health},
};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};
use torrent_port::{
    FetchRequest, NetworkProfile, PeerReachability, ResolveRequest, SeedRequest, TorrentEngine,
    TorrentSource,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zeron-seed-{name}-{}", std::process::id()));
        let _ = make_writable(&path);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = make_writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_writable(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mut permissions = metadata.permissions();
        #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions)?;
        for entry in std::fs::read_dir(path)? {
            make_writable(&entry?.path())?;
        }
    }
    Ok(())
}

struct Release {
    directory: PathBuf,
    torrent: Vec<u8>,
    info_hash: String,
    lecture: Vec<u8>,
}

/// Publish a one-lesson course, the way Publish does.
fn publish(root: &Path) -> Release {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "1.0.0".to_owned(),
                title: "Hub Course".to_owned(),
                publisher_name: "Zeron Tests".to_owned(),
                language: "en".to_owned(),
                ..MetadataEdit::default()
            },
        )
        .unwrap();
    let module_id = drafts
        .edit_structure(
            &draft_id,
            1,
            StructureEdit::AddModule {
                title: "M".to_owned(),
            },
        )
        .unwrap()
        .draft
        .modules[0]
        .id
        .clone();
    let lesson_id = drafts
        .edit_structure(
            &draft_id,
            2,
            StructureEdit::AddLesson {
                module_id,
                title: "L".to_owned(),
            },
        )
        .unwrap()
        .draft
        .modules[0]
        .lessons[0]
        .id
        .clone();
    let lecture: Vec<u8> = (0..2_500_000_u32)
        .map(|index| (index % 251) as u8)
        .collect();
    let source = root.join("lecture.bin");
    std::fs::write(&source, &lecture).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap();
    let draft = drafts.draft(&draft_id).unwrap().unwrap().draft;
    let jobs = PublishJobs::new(&root.join("staging"));
    let job = jobs.begin(&draft).unwrap();
    jobs.run(
        &job.job_id,
        &draft,
        &DistributionOptions::default(),
        &mut |_| {},
    );
    let (record, _) = jobs
        .activate(&job.job_id, &mut drafts, &root.join("releases"))
        .unwrap();
    let directory = root.join("releases").join(&record.release.release_id);
    Release {
        torrent: std::fs::read(directory.join("release.torrent")).unwrap(),
        directory,
        info_hash: record.release.info_hash,
        lecture,
    }
}

async fn engine(root: &Path, name: &str, allow_uploads: bool) -> Arc<LibrqbitEngine> {
    Arc::new(
        LibrqbitEngine::start(EngineConfig {
            profile: NetworkProfile::Loopback,
            state_directory: root.join(name),
            reachability: PeerReachability::default(),
            allow_uploads,
        })
        .await
        .unwrap(),
    )
}

fn config() -> ServeConfig {
    ServeConfig {
        profile: NetworkProfile::Loopback,
        port: None,
        poll: Duration::from_millis(100),
        resolve_timeout: Duration::from_secs(30),
        retry_after: Duration::from_secs(1),
        ..ServeConfig::default()
    }
}

async fn wait_for_state(server: &Server, info_hash: &str, state: &str) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let health = server.health();
        let current = health
            .torrents
            .iter()
            .find(|torrent| torrent.info_hash == info_hash)
            .map(|torrent| torrent.state);
        if current == Some(state) {
            return;
        }
        assert!(Instant::now() < deadline, "still {current:?}: {health:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A learner that may only reach `server` downloads the whole course from it.
async fn learner_downloads_from(root: &Path, name: &str, release: &Release, server: SocketAddr) {
    let learner = engine(root, name, false).await;
    let resolved = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Metainfo(release.torrent.clone()),
            initial_peers: Vec::new(),
        })
        .await
        .unwrap();
    let content = root.join(format!("{name}-content"));
    let info_hash = learner
        .fetch(FetchRequest {
            metainfo: release.torrent.clone(),
            content_root: content.clone(),
            files: resolved.files.iter().map(|file| file.index).collect(),
            initial_peers: vec![server],
        })
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let stats = learner.transfer(&info_hash).unwrap();
        if stats.total_bytes > 0 && stats.verified_bytes == stats.total_bytes {
            break;
        }
        assert!(Instant::now() < deadline, "learner stuck at {stats:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let lecture = resolved
        .files
        .iter()
        .find(|file| file.path.ends_with("lecture.bin"))
        .unwrap();
    assert_eq!(
        std::fs::read(content.join(&lecture.path)).unwrap(),
        release.lecture,
        "the lesson arrives byte for byte"
    );
    learner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_follows_a_magnet_and_keeps_serving_without_the_publisher() {
    let scratch = Scratch::new("hub");
    let release = publish(&scratch.0);
    let publisher = engine(&scratch.0, "publisher", true).await;
    publisher
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.directory.join("package"),
        })
        .await
        .unwrap();
    let publisher_address = publisher.listen_address().unwrap();

    let data = scratch.0.join("server");
    let registry = Registry::open(&data).unwrap();
    registry
        .add_magnet(
            &format!("magnet:?xt=urn:btih:{}&dn=course", release.info_hash),
            vec![publisher_address],
        )
        .unwrap();
    let server = Server::start(registry, config()).await.unwrap();
    wait_for_state(&server, &release.info_hash, "seeding").await;
    let health = server.health();
    assert_eq!(health.torrents[0].mode, "follow");
    assert_eq!(
        health.torrents[0].verified_bytes,
        health.torrents[0].total_bytes
    );

    // The publisher goes offline; learners still get the course from the hub.
    publisher.shutdown().await;
    let hub = server.listen_address().unwrap();
    learner_downloads_from(&scratch.0, "learner-1", &release, hub).await;
    assert!(server.health().torrents[0].uploaded_bytes >= release.lecture.len() as u64);

    // After a restart the hub serves from its own copy, with no peer at all.
    server.shutdown().await;
    let restarted = Server::start(Registry::open(&data).unwrap(), config())
        .await
        .unwrap();
    wait_for_state(&restarted, &release.info_hash, "seeding").await;
    let hub = restarted.listen_address().unwrap();
    learner_downloads_from(&scratch.0, "learner-2", &release, hub).await;
    restarted.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copied_release_is_served_reported_and_dropped_when_removed() {
    let scratch = Scratch::new("release");
    let release = publish(&scratch.0);
    let data = scratch.0.join("server");
    let registry = Registry::open(&data).unwrap();
    let server = Server::start(registry.clone(), config()).await.unwrap();
    assert_eq!(server.health().torrents.len(), 0);

    // Added while serving: the server notices the list changed.
    registry
        .add_release(
            &release.info_hash,
            &release.torrent,
            &release.directory.join("package"),
        )
        .unwrap();
    assert!(
        registry
            .add_release(
                &release.info_hash,
                &release.torrent,
                &release.directory.join("package")
            )
            .is_err()
    );
    server.reconcile().await.unwrap();
    wait_for_state(&server, &release.info_hash, "seeding").await;

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let health = serve_health(Arc::clone(&server), "127.0.0.1:0".parse().unwrap(), async {
        let _ = stopped.await;
    })
    .await
    .unwrap();
    let body = get(health, "/health").await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["torrents"][0]["mode"], "release");
    assert_eq!(json["torrents"][0]["state"], "seeding");
    assert_eq!(
        json["torrents"][0]["infoHash"],
        serde_json::Value::Null,
        "snake_case keys"
    );
    assert_eq!(json["torrents"][0]["info_hash"], release.info_hash);
    assert!(!body.contains(&*scratch.0.to_string_lossy()), "no paths");

    learner_downloads_from(
        &scratch.0,
        "learner",
        &release,
        server.listen_address().unwrap(),
    )
    .await;

    assert!(registry.remove(&release.info_hash).unwrap());
    // Same-second edits can share a modification time; make it differ.
    tokio::time::sleep(Duration::from_millis(20)).await;
    server.reconcile().await.unwrap();
    assert_eq!(server.health().torrents.len(), 0);
    let _ = stop.send(());
    server.shutdown().await;
    assert!(
        release.directory.join("package").is_dir(),
        "release files are never touched"
    );
}

async fn get(address: SocketAddr, path: &str) -> String {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}
