//! Seeding Zeron releases through the port, verified by an independent
//! loopback-only librqbit learner. No public network is used.

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use librqbit::{
    AddTorrent, AddTorrentOptions, ListenerMode, ListenerOptions, Session, SessionOptions,
};
use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, StructureEdit,
};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};
use torrent_port::{
    EngineError, InfoHash, NetworkProfile, SeedRequest, TorrentEngine, TransferState,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zeron-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Release {
    package: PathBuf,
    torrent: Vec<u8>,
    info_hash: String,
}

fn publish(root: &Path) -> Release {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "1.0.0".to_owned(),
                title: "Seeded Course".to_owned(),
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
    let mut revision = 2;
    std::fs::create_dir_all(root.join("sources")).unwrap();
    for (index, length) in [(0_u8, 900_000_usize), (1, 2_500_000), (2, 70_000)] {
        let lesson_id = drafts
            .edit_structure(
                &draft_id,
                revision,
                StructureEdit::AddLesson {
                    module_id: module_id.clone(),
                    title: format!("L{index}"),
                },
            )
            .unwrap()
            .draft
            .modules[0]
            .lessons[usize::from(index)]
        .id
        .clone();
        revision += 1;
        let source = root.join("sources").join(format!("lesson-{index}.bin"));
        let bytes: Vec<u8> = (0..length)
            .map(|position| position.to_le_bytes()[0] ^ position.to_le_bytes()[1] ^ index)
            .collect();
        std::fs::write(&source, bytes).unwrap();
        drafts
            .attach_source(&draft_id, revision, &lesson_id, &source)
            .unwrap();
        revision += 1;
    }
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
        package: directory.join("package"),
        torrent: std::fs::read(directory.join("release.torrent")).unwrap(),
        info_hash: record.release.info_hash,
    }
}

async fn engine(root: &Path) -> LibrqbitEngine {
    LibrqbitEngine::start(EngineConfig {
        profile: NetworkProfile::Loopback,
        state_directory: root.join("engine"),
        listen_port: None,
    })
    .await
    .unwrap()
}

async fn wait_for_state(
    engine: &LibrqbitEngine,
    info_hash: &InfoHash,
    wanted: TransferState,
) -> torrent_port::TransferStats {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let stats = engine.transfer(info_hash).expect("transfer is active");
        if stats.state == wanted {
            return stats;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {wanted:?}: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn learner_options() -> SessionOptions {
    SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        persistence: None,
        ipv4_only: true,
        bind_device_name: cfg!(target_os = "linux").then(|| "lo".to_owned()),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpOnly,
            listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            enable_upnp_port_forwarding: false,
            ipv4_only: true,
            ..ListenerOptions::default()
        }),
        ..SessionOptions::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_release_is_seeded_to_a_local_learner_without_modification() {
    let scratch = Scratch::new("seed");
    let release = publish(&scratch.0);
    let before = snapshot(&release.package);
    let engine = engine(&scratch.0).await;
    let address = engine.listen_address().unwrap();
    assert!(address.ip().is_loopback());

    let info_hash = engine
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    assert_eq!(info_hash.as_str(), release.info_hash);
    let seeding = wait_for_state(&engine, &info_hash, TransferState::Seeding).await;
    assert_eq!(seeding.verified_bytes, seeding.total_bytes);
    assert!(seeding.failure.is_none());
    assert_eq!(
        engine.transfers(),
        vec![engine.transfer(&info_hash).unwrap()]
    );
    assert!(matches!(
        engine
            .seed(SeedRequest {
                torrent: release.torrent.clone(),
                content_root: release.package.clone(),
            })
            .await,
        Err(EngineError::AlreadyActive)
    ));

    let learner = Session::new_with_opts(scratch.0.join("learner-state"), learner_options())
        .await
        .unwrap();
    let output = scratch.0.join("learner");
    let handle = learner
        .add_torrent(
            AddTorrent::from_bytes(release.torrent.clone()),
            Some(AddTorrentOptions {
                initial_peers: Some(vec![address]),
                output_folder: Some(output.to_string_lossy().into_owned()),
                ..AddTorrentOptions::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), handle.wait_until_completed())
        .await
        .unwrap()
        .unwrap();
    for (path, _, bytes) in &before {
        let relative = path.strip_prefix(&release.package).unwrap();
        assert_eq!(&std::fs::read(output.join(relative)).unwrap(), bytes);
    }
    learner.stop().await;

    let served = engine.transfer(&info_hash).unwrap();
    assert!(served.uploaded_bytes >= served.total_bytes);
    engine.stop(&info_hash).await.unwrap();
    assert!(engine.transfer(&info_hash).is_none());
    assert!(matches!(
        engine.stop(&info_hash).await,
        Err(EngineError::NotActive)
    ));
    assert_eq!(snapshot(&release.package), before);
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_content_fails_closed_and_missing_content_is_rejected() {
    let scratch = Scratch::new("tamper");
    let release = publish(&scratch.0);
    let copy = scratch.0.join("copy");
    copy_tree(&release.package, &copy);
    make_writable(&copy);
    let victim = snapshot(&copy)
        .into_iter()
        .map(|(path, _, _)| path)
        .find(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .unwrap();
    let mut bytes = std::fs::read(&victim).unwrap();
    bytes[1_000] ^= 0xff;
    std::fs::write(&victim, &bytes).unwrap();

    let engine = engine(&scratch.0).await;
    let info_hash = engine
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: copy.clone(),
        })
        .await
        .unwrap();
    let failed = wait_for_state(&engine, &info_hash, TransferState::Failed).await;
    assert!(failed.verified_bytes < failed.total_bytes);
    assert!(failed.failure.unwrap().contains("no longer matches"));
    assert_eq!(std::fs::read(&victim).unwrap(), bytes);
    engine.stop(&info_hash).await.unwrap();

    std::fs::remove_file(&victim).unwrap();
    assert!(matches!(
        engine
            .seed(SeedRequest {
                torrent: release.torrent.clone(),
                content_root: copy,
            })
            .await,
        Err(EngineError::ContentMismatch(message)) if message.contains("missing")
    ));
    assert!(matches!(
        engine
            .seed(SeedRequest {
                torrent: b"not a torrent".to_vec(),
                content_root: release.package.clone(),
            })
            .await,
        Err(EngineError::InvalidTorrent(_))
    ));
    assert!(engine.transfers().is_empty());
    engine.shutdown().await;
}

fn snapshot(root: &Path) -> Vec<(PathBuf, bool, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let readonly = std::fs::metadata(&path).unwrap().permissions().readonly();
                let bytes = std::fs::read(&path).unwrap();
                entries.push((path, readonly, bytes));
            }
        }
    }
    entries.sort();
    entries
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn make_writable(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            make_writable(&path);
        } else if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}
