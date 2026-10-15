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
use torrent_librqbit::{EngineConfig, LibrqbitEngine, PeerReachability};
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
    publish_lessons(root, &[900_000, 2_500_000, 70_000])
}

fn publish_lessons(root: &Path, lengths: &[usize]) -> Release {
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
    for (index, &length) in (0_u8..).zip(lengths) {
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
        reachability: PeerReachability::default(),
        allow_uploads: true,
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
    assert!(matches!(
        engine.resume(&info_hash).await,
        Err(EngineError::Engine(_))
    ));
    assert_eq!(
        engine.transfer(&info_hash).unwrap().state,
        TransferState::Failed
    );
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
    assert_eq!(engine.transfers().len(), 0);
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

#[tokio::test(flavor = "multi_thread")]
async fn learners_resolve_metadata_and_fetch_only_selected_files() {
    use torrent_port::{FetchRequest, ResolveRequest, TorrentSource};

    let scratch = Scratch::new("fetch");
    let release = publish(&scratch.0);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let seeder_address = seeder.listen_address().unwrap();

    let learner = LibrqbitEngine::start(EngineConfig {
        profile: NetworkProfile::Loopback,
        state_directory: scratch.0.join("learner-engine"),
        reachability: PeerReachability::default(),
        allow_uploads: false,
    })
    .await
    .unwrap();
    let offline = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Metainfo(release.torrent.clone()),
            initial_peers: vec![],
        })
        .await
        .unwrap();
    assert_eq!(offline.info_hash.as_str(), release.info_hash);
    assert_eq!(offline.files.len(), 4);
    assert_eq!(offline.files[0].path, "course.json");

    let magnet = format!("magnet:?xt=urn:btih:{}", release.info_hash);
    let resolved = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Magnet(magnet),
            initial_peers: vec![seeder_address],
        })
        .await
        .unwrap();
    assert_eq!(resolved.info_hash, offline.info_hash);
    assert_eq!(resolved.files, offline.files);
    assert!(matches!(
        learner
            .resolve(ResolveRequest {
                source: TorrentSource::Magnet("magnet:?dn=nothing".to_owned()),
                initial_peers: vec![],
            })
            .await,
        Err(EngineError::InvalidTorrent(_))
    ));

    let content = scratch.0.join("learner-cache");
    let fetched = learner
        .fetch(FetchRequest {
            metainfo: resolved.metainfo.clone(),
            content_root: content.clone(),
            files: vec![0],
            initial_peers: vec![seeder_address],
        })
        .await
        .unwrap();
    let manifest_length = resolved.files[0].length;
    let stats = wait_for_file(&learner, &fetched, 0, manifest_length).await;
    assert_eq!(stats.file_verified_bytes.len(), 4);
    let largest = resolved
        .files
        .iter()
        .max_by_key(|file| file.length)
        .unwrap();
    assert!(stats.file_verified_bytes[largest.index] < largest.length / 2);
    assert_eq!(
        std::fs::read(content.join("course.json")).unwrap(),
        std::fs::read(release.package.join("course.json")).unwrap()
    );

    learner
        .fetch(FetchRequest {
            metainfo: resolved.metainfo.clone(),
            content_root: content.clone(),
            files: vec![0, largest.index],
            initial_peers: vec![seeder_address],
        })
        .await
        .unwrap();
    wait_for_file(&learner, &fetched, largest.index, largest.length).await;
    learner.shutdown().await;
    seeder.shutdown().await;
}

async fn wait_for_file(
    engine: &LibrqbitEngine,
    info_hash: &InfoHash,
    index: usize,
    length: u64,
) -> torrent_port::TransferStats {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let stats = engine.transfer(info_hash).expect("transfer is active");
        if stats.file_verified_bytes.get(index) == Some(&length) {
            return stats;
        }
        assert!(
            Instant::now() < deadline,
            "file {index} incomplete: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn magnet_resolution_leaves_no_transfer_or_content_behind() {
    use torrent_port::{ResolveRequest, TorrentSource};

    let scratch = Scratch::new("resolve-clean");
    let release = publish(&scratch.0);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let magnet = || TorrentSource::Magnet(format!("magnet:?xt=urn:btih:{}", release.info_hash));

    let learner = LibrqbitEngine::start(EngineConfig {
        profile: NetworkProfile::Loopback,
        state_directory: scratch.0.join("learner-engine"),
        reachability: PeerReachability::default(),
        allow_uploads: false,
    })
    .await
    .unwrap();
    let resolved = learner
        .resolve(ResolveRequest {
            source: magnet(),
            initial_peers: vec![seeder.listen_address().unwrap()],
        })
        .await
        .unwrap();
    assert_eq!(resolved.info_hash, info_hash);
    let seeder_address = seeder.listen_address().unwrap();
    assert!(
        resolved.peers.iter().all(|peer| *peer == seeder_address),
        "peer hints name only the metadata peer: {:?}",
        resolved.peers
    );
    assert!(learner.transfers().is_empty(), "no transfer is left behind");
    let written: Vec<_> = resolved
        .files
        .iter()
        .flat_map(|file| files_named(&scratch.0.join("learner-engine"), &file.path))
        .collect();
    assert!(written.is_empty(), "resolving wrote {written:?}");

    // An engine already holding the torrent answers without any peer.
    let own = seeder
        .resolve(ResolveRequest {
            source: magnet(),
            initial_peers: vec![],
        })
        .await
        .unwrap();
    assert_eq!(own.files, resolved.files);
    assert_eq!(
        seeder.transfer(&info_hash).unwrap().state,
        TransferState::Seeding
    );
    learner.shutdown().await;
    seeder.shutdown().await;
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                files_named(&path, name)
            } else if entry.file_name() == name {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

/// Uses the Local-network profile, so it multicasts local service discovery on
/// every interface of this machine. Opt in with `cargo test -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "announces on the local network; run manually with --ignored"]
async fn magnets_resolve_through_local_service_discovery_alone() {
    use torrent_port::{ResolveRequest, TorrentSource};

    let scratch = Scratch::new("lsd");
    let release = publish(&scratch.0);
    let local = |name: &str, allow_uploads: bool| EngineConfig {
        profile: NetworkProfile::LocalNetwork,
        state_directory: scratch.0.join(name),
        reachability: PeerReachability::default(),
        allow_uploads,
    };
    let seeder = LibrqbitEngine::start(local("seeder-engine", true))
        .await
        .unwrap();
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    // Let the seeder's own start-up announcement pass, as when a learner joins later.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let learner = LibrqbitEngine::start(local("learner-engine", false))
        .await
        .unwrap();
    let started = Instant::now();
    let resolved = tokio::time::timeout(
        Duration::from_secs(60),
        learner.resolve(ResolveRequest {
            source: TorrentSource::Magnet(format!("magnet:?xt=urn:btih:{}", release.info_hash)),
            initial_peers: vec![],
        }),
    )
    .await
    .expect("the learner found no seeder through local service discovery")
    .unwrap();
    assert_eq!(resolved.info_hash, info_hash);
    eprintln!(
        "resolved through local discovery in {:?} with peer hints {:?}",
        started.elapsed(),
        resolved.peers
    );
    learner.shutdown().await;

    // Trackers in the magnet are ignored under this profile and must not slow
    // the resolve down.
    let trackers = (0..70).fold(String::new(), |mut trackers, index| {
        use std::fmt::Write as _;
        let _ = write!(
            trackers,
            "&tr=udp%3A%2F%2Ftracker{index}.example%3A6969%2Fannounce"
        );
        trackers
    });
    let learner = LibrqbitEngine::start(local("learner-engine-trackers", false))
        .await
        .unwrap();
    let started = Instant::now();
    let resolved = tokio::time::timeout(
        Duration::from_secs(120),
        learner.resolve(ResolveRequest {
            source: TorrentSource::Magnet(format!(
                "magnet:?xt=urn:btih:{}{trackers}",
                release.info_hash
            )),
            initial_peers: vec![],
        }),
    )
    .await
    .expect("the learner found no seeder with trackers listed")
    .unwrap();
    eprintln!(
        "with {} trackers listed, resolved in {:?}",
        trackers.matches("&tr=").count(),
        started.elapsed()
    );
    assert_eq!(resolved.info_hash, info_hash);
    learner.shutdown().await;
    seeder.shutdown().await;
}

/// Binds a fixed port on every interface under the Local-network profile.
/// Opt in with `cargo test -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "listens on the local network; run manually with --ignored"]
async fn a_fixed_peer_port_is_bound_under_the_local_network_profile() {
    let scratch = Scratch::new("fixed-port");
    let port = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let engine = LibrqbitEngine::start(EngineConfig {
        profile: NetworkProfile::LocalNetwork,
        state_directory: scratch.0.join("engine"),
        reachability: PeerReachability::new(Some(port), false).unwrap(),
        allow_uploads: true,
    })
    .await
    .unwrap();
    assert_eq!(engine.listen_address().unwrap().port(), port);
    assert!(
        std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_err(),
        "the engine does not hold port {port}"
    );
    engine.shutdown().await;
}

async fn loopback(root: &Path, name: &str, allow_uploads: bool) -> LibrqbitEngine {
    LibrqbitEngine::start(EngineConfig {
        profile: NetworkProfile::Loopback,
        state_directory: root.join(name),
        reachability: PeerReachability::default(),
        allow_uploads,
    })
    .await
    .unwrap()
}

async fn fetch_manifest(
    engine: &LibrqbitEngine,
    release: &Release,
    content_root: PathBuf,
    peer: SocketAddr,
) -> InfoHash {
    engine
        .fetch(torrent_port::FetchRequest {
            metainfo: release.torrent.clone(),
            content_root,
            files: vec![0],
            initial_peers: vec![peer],
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn engines_with_uploads_disabled_serve_no_pieces() {
    let scratch = Scratch::new("no-upload");
    let release = publish(&scratch.0);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let seeder_address = seeder.listen_address().unwrap();

    let private = loopback(&scratch.0, "private", false).await;
    assert!(matches!(
        private
            .seed(SeedRequest {
                torrent: release.torrent.clone(),
                content_root: release.package.clone(),
            })
            .await,
        Err(EngineError::UnsupportedCapability(_))
    ));
    let sharing = loopback(&scratch.0, "sharing", true).await;
    let manifest_length = torrent_metainfo_length(&release);
    for (learner, name) in [(&private, "private-content"), (&sharing, "sharing-content")] {
        fetch_manifest(learner, &release, scratch.0.join(name), seeder_address).await;
        wait_for_file(learner, &info_hash, 0, manifest_length).await;
    }
    seeder.shutdown().await;

    // Only the private learner holds course.json now: nothing reaches a peer.
    let from_private = loopback(&scratch.0, "from-private", false).await;
    let address = private.listen_address().unwrap();
    fetch_manifest(&from_private, &release, scratch.0.join("a"), address).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let stats = from_private.transfer(&info_hash).unwrap();
    assert_eq!(stats.file_verified_bytes[0], 0, "{stats:?}");
    assert_eq!(private.transfer(&info_hash).unwrap().uploaded_bytes, 0);

    // The same request to a sharing learner completes.
    let from_sharing = loopback(&scratch.0, "from-sharing", false).await;
    let address = sharing.listen_address().unwrap();
    fetch_manifest(&from_sharing, &release, scratch.0.join("b"), address).await;
    wait_for_file(&from_sharing, &info_hash, 0, manifest_length).await;
    for engine in [private, sharing, from_private, from_sharing] {
        engine.shutdown().await;
    }
}

fn torrent_metainfo_length(release: &Release) -> u64 {
    std::fs::metadata(release.package.join("course.json"))
        .unwrap()
        .len()
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_verified_bytes_from_any_offset_without_downloading_the_rest() {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    use torrent_port::{FetchRequest, ResolveRequest, TorrentSource};

    const LESSON: usize = 100_000_000;
    const SLICE: usize = 1 << 20;
    let scratch = Scratch::new("stream");
    let release = publish_lessons(&scratch.0, &[LESSON]);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let seeder_address = seeder.listen_address().unwrap();

    let learner = loopback(&scratch.0, "learner", false).await;
    let resolved = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Metainfo(release.torrent.clone()),
            initial_peers: vec![],
        })
        .await
        .unwrap();
    let lesson = resolved
        .files
        .iter()
        .find(|file| file.path != "course.json")
        .unwrap()
        .clone();
    assert!(matches!(
        learner.open_file(&info_hash, lesson.index).await,
        Err(EngineError::NotActive)
    ));
    learner
        .fetch(FetchRequest {
            metainfo: release.torrent.clone(),
            content_root: scratch.0.join("learner-content"),
            files: vec![0],
            initial_peers: vec![seeder_address],
        })
        .await
        .unwrap();
    wait_for_file(&learner, &info_hash, 0, resolved.files[0].length).await;
    assert!(matches!(
        learner.open_file(&info_hash, resolved.files.len()).await,
        Err(EngineError::InvalidTorrent(_))
    ));
    let source = std::fs::read(release.package.join(&lesson.path)).unwrap();

    // Seek far into the unselected lesson: only a window around it is fetched.
    let offset = 80_000_000;
    let mut reader = learner.open_file(&info_hash, lesson.index).await.unwrap();
    reader.seek(std::io::SeekFrom::Start(offset)).await.unwrap();
    let mut slice = vec![0_u8; SLICE];
    tokio::time::timeout(Duration::from_secs(30), reader.read_exact(&mut slice))
        .await
        .expect("verified bytes arrive")
        .unwrap();
    let start = usize::try_from(offset).unwrap();
    assert!(
        slice == source[start..start + SLICE],
        "streamed bytes differ"
    );
    let verified = learner.transfer(&info_hash).unwrap().file_verified_bytes[lesson.index];
    assert!(
        verified < 40_000_000,
        "only the lookahead window is fetched: {verified}"
    );
    drop(reader);

    // A new reader from the start gets earlier bytes on demand.
    let mut reader = learner.open_file(&info_hash, lesson.index).await.unwrap();
    tokio::time::timeout(Duration::from_secs(30), reader.read_exact(&mut slice))
        .await
        .expect("verified bytes arrive")
        .unwrap();
    assert!(slice == source[..SLICE], "streamed bytes differ");
    drop(reader);
    learner.shutdown().await;
    seeder.shutdown().await;
}

/// A torrent is added in an initializing state while librqbit checks the
/// bytes already on disk. A stream opened in that window must wait for the
/// check instead of failing, as happens when a learner reopens a lesson right
/// after the app restores its courses at startup.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_opened_while_the_torrent_is_still_checking_waits_for_it() {
    use tokio::io::AsyncReadExt;

    const LESSON: usize = 100_000_000;
    let scratch = Scratch::new("stream-checking");
    let release = publish_lessons(&scratch.0, &[LESSON]);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    let checking = seeder.transfer(&info_hash).unwrap().state;
    let mut reader = tokio::time::timeout(Duration::from_secs(60), seeder.open_file(&info_hash, 1))
        .await
        .expect("the check finishes")
        .expect("a stream opened during the check waits for it");
    let mut start = vec![0_u8; 4096];
    reader.read_exact(&mut start).await.unwrap();
    let lesson = std::fs::read_dir(release.package.join("resources"))
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().path())
        .find(|path| std::fs::metadata(path).unwrap().len() == LESSON as u64)
        .unwrap();
    assert_eq!(start, std::fs::read(lesson).unwrap()[..4096]);
    assert_eq!(checking, TransferState::Checking, "the race was exercised");
    drop(reader);
    seeder.shutdown().await;
}

/// When `course.json` completes, a learner with uploads off disconnects its
/// peers. A stream opened while that disconnect is in progress used to wait
/// forever; the reader now asks for the peers again while it waits.
#[tokio::test(flavor = "multi_thread")]
async fn streams_reconnect_peers_dropped_when_the_selection_finished() {
    use tokio::io::AsyncReadExt;

    let scratch = Scratch::new("reconnect");
    let release = publish(&scratch.0);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let seeder_address = seeder.listen_address().unwrap();
    let manifest_length = torrent_metainfo_length(&release);
    for delay in [0, 100, 200, 300, 500] {
        let learner = loopback(&scratch.0, &format!("learner-{delay}"), false).await;
        fetch_manifest(
            &learner,
            &release,
            scratch.0.join(format!("content-{delay}")),
            seeder_address,
        )
        .await;
        wait_for_file(&learner, &info_hash, 0, manifest_length).await;
        tokio::time::sleep(Duration::from_millis(delay)).await;
        let mut reader = learner.open_file(&info_hash, 1).await.unwrap();
        let mut head = vec![0_u8; 64 * 1024];
        tokio::time::timeout(Duration::from_secs(20), reader.read_exact(&mut head))
            .await
            .unwrap_or_else(|_| panic!("stream stalled after a {delay} ms delay"))
            .unwrap();
        drop(reader);
        learner.shutdown().await;
    }
    seeder.shutdown().await;
}

fn lesson_source(release: &Release, suffix: &str) -> Vec<u8> {
    let path = std::fs::read_dir(release.package.join("resources"))
        .unwrap()
        .flat_map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(suffix))
        .unwrap();
    std::fs::read(path).unwrap()
}

async fn read_at(
    reader: &mut Box<dyn torrent_port::VerifiedFile>,
    offset: u64,
    length: usize,
) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    reader.seek(std::io::SeekFrom::Start(offset)).await.unwrap();
    let mut bytes = vec![0_u8; length];
    tokio::time::timeout(Duration::from_secs(60), reader.read_exact(&mut bytes))
        .await
        .expect("verified bytes arrive")
        .unwrap();
    bytes
}

/// The roadmap's first milestone in a controlled loopback swarm: lesson four
/// plays before it is complete, a seek into data not yet downloaded resumes,
/// and a restarted learner reuses the bytes it already verified.
#[tokio::test(flavor = "multi_thread")]
async fn the_first_streaming_milestone_holds_in_a_controlled_swarm() {
    use torrent_port::{FetchRequest, ResolveRequest, TorrentSource};

    const LESSON: usize = 24_000_000;
    const SLICE: usize = 1 << 20;
    let scratch = Scratch::new("milestone");
    let release = publish_lessons(&scratch.0, &[LESSON; 5]);
    let source = lesson_source(&release, "lesson-3.bin");
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let seeder_address = seeder.listen_address().unwrap();

    let learner = loopback(&scratch.0, "learner", false).await;
    let resolved = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Metainfo(release.torrent.clone()),
            initial_peers: vec![],
        })
        .await
        .unwrap();
    let lesson = resolved
        .files
        .iter()
        .find(|file| file.path.ends_with("lesson-3.bin"))
        .unwrap()
        .index;
    let content_root = scratch.0.join("learner-content");
    fetch_manifest(&learner, &release, content_root.clone(), seeder_address).await;
    wait_for_file(&learner, &info_hash, 0, resolved.files[0].length).await;

    // Lesson four plays before it is complete.
    let mut reader = learner.open_file(&info_hash, lesson).await.unwrap();
    assert!(
        read_at(&mut reader, 0, SLICE).await == source[..SLICE],
        "streamed bytes differ"
    );
    let started = learner.transfer(&info_hash).unwrap().file_verified_bytes[lesson];
    assert!(
        started < LESSON as u64 / 2,
        "playing needs only the start: {started}"
    );

    // A seek into data not yet downloaded resumes once it is fetched.
    let offset = LESSON * 9 / 10;
    let before_seek = learner.transfer(&info_hash).unwrap().file_verified_bytes[lesson];
    assert!(
        before_seek < LESSON as u64,
        "the seek target is not downloaded yet"
    );
    assert!(
        read_at(&mut reader, offset as u64, SLICE).await == source[offset..offset + SLICE],
        "streamed bytes differ"
    );
    drop(reader);
    let verified = learner.transfer(&info_hash).unwrap().file_verified_bytes[lesson];
    assert!(
        verified > 0 && verified < LESSON as u64,
        "partly verified: {verified}"
    );
    learner.shutdown().await;
    seeder.shutdown().await;

    // With no peer left, a restarted learner still has what it verified.
    let restarted = loopback(&scratch.0, "learner", false).await;
    restarted
        .fetch(FetchRequest {
            metainfo: release.torrent.clone(),
            content_root,
            files: vec![0],
            initial_peers: vec![],
        })
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let reused = loop {
        let stats = restarted.transfer(&info_hash).unwrap();
        if stats.state != TransferState::Checking {
            break stats.file_verified_bytes[lesson];
        }
        assert!(Instant::now() < deadline, "the restart check finishes");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        reused >= verified,
        "verified bytes reused: {reused} of {verified}"
    );
    let mut reader = restarted.open_file(&info_hash, lesson).await.unwrap();
    assert!(
        read_at(&mut reader, 0, SLICE).await == source[..SLICE],
        "streamed bytes differ"
    );
    assert!(
        read_at(&mut reader, offset as u64, SLICE).await == source[offset..offset + SLICE],
        "streamed bytes differ"
    );
    drop(reader);
    restarted.shutdown().await;
}

/// A stream keeps going when one of its two seeders disappears.
#[tokio::test(flavor = "multi_thread")]
async fn a_stream_survives_losing_one_of_two_seeders() {
    use tokio::io::AsyncReadExt;
    use torrent_port::FetchRequest;

    const LESSON: usize = 40_000_000;
    let scratch = Scratch::new("peer-loss");
    let release = publish_lessons(&scratch.0, &[LESSON]);
    let source = lesson_source(&release, "lesson-0.bin");
    let first = loopback(&scratch.0, "seeder-a", true).await;
    let second = loopback(&scratch.0, "seeder-b", true).await;
    let mut info_hash = None;
    for seeder in [&first, &second] {
        let hash = seeder
            .seed(SeedRequest {
                torrent: release.torrent.clone(),
                content_root: release.package.clone(),
            })
            .await
            .unwrap();
        wait_for_state(seeder, &hash, TransferState::Seeding).await;
        info_hash = Some(hash);
    }
    let info_hash = info_hash.unwrap();
    let learner = loopback(&scratch.0, "learner", false).await;
    learner
        .fetch(FetchRequest {
            metainfo: release.torrent.clone(),
            content_root: scratch.0.join("learner-content"),
            files: vec![0],
            initial_peers: vec![
                first.listen_address().unwrap(),
                second.listen_address().unwrap(),
            ],
        })
        .await
        .unwrap();
    let mut reader = learner.open_file(&info_hash, 1).await.unwrap();
    let mut streamed = vec![0_u8; 4 << 20];
    tokio::time::timeout(Duration::from_secs(60), reader.read_exact(&mut streamed))
        .await
        .expect("the stream starts")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while learner.transfer(&info_hash).unwrap().connected_peers < 2 {
        assert!(
            Instant::now() < deadline,
            "the learner reaches both seeders"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    first.shutdown().await;

    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(120), reader.read_to_end(&mut rest))
        .await
        .expect("the remaining seeder serves the rest")
        .unwrap();
    streamed.extend(rest);
    assert_eq!(streamed.len(), LESSON);
    assert!(streamed == source, "streamed bytes differ");
    drop(reader);
    learner.shutdown().await;
    second.shutdown().await;
}

/// Selecting a lesson downloads it in full without a reader; other lessons
/// stay unselected apart from shared boundary pieces, and deselecting keeps
/// what was verified.
#[tokio::test(flavor = "multi_thread")]
async fn selected_files_download_in_full_and_stay_after_deselection() {
    use torrent_port::{ResolveRequest, TorrentSource};

    const LESSON: usize = 6_000_000;
    let scratch = Scratch::new("select");
    let release = publish_lessons(&scratch.0, &[LESSON; 3]);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let learner = loopback(&scratch.0, "learner", false).await;
    let resolved = learner
        .resolve(ResolveRequest {
            source: TorrentSource::Metainfo(release.torrent.clone()),
            initial_peers: vec![],
        })
        .await
        .unwrap();
    let index_of = |suffix: &str| {
        resolved
            .files
            .iter()
            .find(|file| file.path.ends_with(suffix))
            .unwrap()
            .index
    };
    let (chosen, others) = (
        index_of("lesson-1.bin"),
        [index_of("lesson-0.bin"), index_of("lesson-2.bin")],
    );

    assert!(matches!(
        learner.select_files(&info_hash, &[0]).await,
        Err(EngineError::NotActive)
    ));
    fetch_manifest(
        &learner,
        &release,
        scratch.0.join("learner-content"),
        seeder.listen_address().unwrap(),
    )
    .await;
    wait_for_file(&learner, &info_hash, 0, resolved.files[0].length).await;
    assert!(matches!(
        learner
            .select_files(&info_hash, &[resolved.files.len()])
            .await,
        Err(EngineError::InvalidTorrent(_))
    ));
    assert!(matches!(
        learner.select_files(&info_hash, &[]).await,
        Err(EngineError::InvalidTorrent(_))
    ));

    learner
        .select_files(&info_hash, &[0, chosen])
        .await
        .unwrap();
    let stats = wait_for_file(&learner, &info_hash, chosen, LESSON as u64).await;
    for other in others {
        assert!(
            stats.file_verified_bytes[other] < LESSON as u64 / 2,
            "only boundary pieces of unselected lessons: {stats:?}"
        );
    }

    learner.select_files(&info_hash, &[0]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        learner.transfer(&info_hash).unwrap().file_verified_bytes[chosen],
        LESSON as u64
    );
    learner.shutdown().await;
    seeder.shutdown().await;
}

/// A paused transfer downloads nothing until resumed, keeps serving what it
/// already verified to a local reader, and finishes after resuming.
#[tokio::test(flavor = "multi_thread")]
async fn paused_transfers_download_nothing_until_resumed() {
    use tokio::io::AsyncReadExt;

    const LESSON: usize = 8_000_000;
    let scratch = Scratch::new("pause");
    let release = publish_lessons(&scratch.0, &[LESSON]);
    let seeder = engine(&scratch.0).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
        })
        .await
        .unwrap();
    wait_for_state(&seeder, &info_hash, TransferState::Seeding).await;
    let learner = loopback(&scratch.0, "learner", false).await;
    assert!(matches!(
        learner.pause(&info_hash).await,
        Err(EngineError::NotActive)
    ));
    fetch_manifest(
        &learner,
        &release,
        scratch.0.join("learner-content"),
        seeder.listen_address().unwrap(),
    )
    .await;
    let manifest_length = std::fs::metadata(release.package.join("course.json"))
        .unwrap()
        .len();
    wait_for_file(&learner, &info_hash, 0, manifest_length).await;

    learner.pause(&info_hash).await.unwrap();
    learner.pause(&info_hash).await.unwrap();
    assert_eq!(
        learner.transfer(&info_hash).unwrap().state,
        TransferState::Paused
    );
    learner.select_files(&info_hash, &[0, 1]).await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let paused = learner.transfer(&info_hash).unwrap();
    assert_eq!(paused.state, TransferState::Paused);
    assert!(
        paused.file_verified_bytes[1] < LESSON as u64 / 2,
        "nothing downloads while paused: {paused:?}"
    );
    let mut manifest = learner.open_file(&info_hash, 0).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), manifest.read_to_end(&mut bytes))
        .await
        .expect("verified bytes stay readable while paused")
        .unwrap();
    assert_eq!(
        bytes,
        std::fs::read(release.package.join("course.json")).unwrap()
    );
    drop(manifest);

    learner.resume(&info_hash).await.unwrap();
    learner.resume(&info_hash).await.unwrap();
    wait_for_file(&learner, &info_hash, 1, LESSON as u64).await;
    assert_ne!(
        learner.transfer(&info_hash).unwrap().state,
        TransferState::Paused
    );
    learner.shutdown().await;
    seeder.shutdown().await;
}
