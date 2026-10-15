//! Metadata-first torrent import against real loopback engines and a scripted engine.

use std::{
    future::pending,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use learning_application::{
    CourseLibrary, CourseSharing, ImportLimits, SharingError, TorrentImportSnapshot,
    TorrentImportStage, TorrentImportTimeouts, TorrentImports, TorrentSource, TransferRouter,
};
use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, StructureEdit,
};
use torrent_librqbit::{EngineConfig, LibrqbitEngine, PeerReachability};
use torrent_port::{
    EngineError, FetchRequest, InfoHash, NetworkProfile, ResolveRequest, ResolvedTorrent,
    SeedRequest, TorrentEngine, TorrentFile, TransferState, TransferStats,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zeron-torrent-import-{name}-{}",
            std::process::id()
        ));
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
    title: String,
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
                title: "Imported Over Torrent".to_owned(),
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
    let source = root.join("lecture.bin");
    std::fs::write(&source, vec![4_u8; 3_000_000]).unwrap();
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
        package: directory.join("package"),
        torrent: std::fs::read(directory.join("release.torrent")).unwrap(),
        info_hash: record.release.info_hash,
        title: record.release.title,
    }
}

async fn loopback_engine(root: &Path, name: &str, allow_uploads: bool) -> Arc<LibrqbitEngine> {
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

async fn seed(engine: &LibrqbitEngine, torrent: Vec<u8>, content_root: PathBuf) -> SocketAddr {
    let info_hash = engine
        .seed(SeedRequest {
            torrent,
            content_root,
        })
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while engine.transfer(&info_hash).unwrap().state != TransferState::Seeding {
        assert!(Instant::now() < deadline, "seeder never became ready");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    engine.listen_address().unwrap()
}

fn imports<E: TorrentEngine>(
    engine: Arc<E>,
    library: &Arc<Mutex<CourseLibrary>>,
    courses_root: PathBuf,
    timeouts: TorrentImportTimeouts,
) -> TorrentImports<E> {
    TorrentImports::new(
        engine,
        Arc::clone(library),
        courses_root,
        ImportLimits::default(),
        timeouts,
    )
}

fn fast_timeouts() -> TorrentImportTimeouts {
    TorrentImportTimeouts {
        metadata: Duration::from_millis(400),
        stall: Duration::from_millis(400),
        poll: Duration::from_millis(20),
    }
}

async fn run<E: TorrentEngine>(
    imports: &TorrentImports<E>,
    source: TorrentSource,
    peers: Vec<SocketAddr>,
) -> (TorrentImportSnapshot, Vec<TorrentImportStage>) {
    let job = imports.begin().unwrap();
    let mut stages = Vec::new();
    let finished = imports
        .run(&job.job_id, source, peers, &mut |snapshot| {
            if stages.last() != Some(&snapshot.stage) {
                stages.push(snapshot.stage);
            }
        })
        .await
        .unwrap();
    (finished, stages)
}

/// The imported lesson maps to its torrent file and streams verified bytes
/// identical to the published package.
async fn assert_lesson_streams(
    learner: &LibrqbitEngine,
    library: &Arc<Mutex<CourseLibrary>>,
    course_id: &str,
    release: &Release,
    info_hash: &InfoHash,
) {
    let resources = library
        .lock()
        .unwrap()
        .torrent_resources(course_id)
        .unwrap();
    assert_eq!(resources.len(), 1);
    let resource = &resources[0];
    assert_eq!(resource.info_hash, release.info_hash);
    let mut reader = learner
        .open_file(info_hash, resource.file_index)
        .await
        .unwrap();
    let mut head = vec![0_u8; 64 * 1024];
    tokio::time::timeout(
        Duration::from_secs(30),
        tokio::io::AsyncReadExt::read_exact(&mut reader, &mut head),
    )
    .await
    .expect("verified lesson bytes stream")
    .unwrap();
    let published = std::fs::read(
        release.package.join(
            library
                .lock()
                .unwrap()
                .manifest(course_id)
                .unwrap()
                .unwrap()
                .modules[0]
                .lessons[0]
                .resources[0]
                .path
                .clone(),
        ),
    )
    .unwrap();
    assert!(head == published[..head.len()], "streamed bytes differ");
    drop(reader);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_magnet_import_fetches_only_the_manifest_and_restores_after_restart() {
    let scratch = Scratch::new("magnet");
    let release = publish(&scratch.0);
    let seeder = loopback_engine(&scratch.0, "seeder", true).await;
    let address = seed(&seeder, release.torrent.clone(), release.package.clone()).await;

    let library = Arc::new(Mutex::new(
        CourseLibrary::open(&scratch.0.join("library.sqlite3"), ImportLimits::default()).unwrap(),
    ));
    let learner = loopback_engine(&scratch.0, "learner", false).await;
    let courses = scratch.0.join("courses");
    let importer = imports(
        Arc::clone(&learner),
        &library,
        courses.clone(),
        TorrentImportTimeouts::default(),
    );
    let magnet = format!("magnet:?xt=urn:btih:{}", release.info_hash);
    let (finished, stages) = run(&importer, TorrentSource::Magnet(magnet), vec![address]).await;

    assert_eq!(finished.stage, TorrentImportStage::Imported, "{finished:?}");
    assert_eq!(
        stages,
        [
            TorrentImportStage::ResolvingMetadata,
            TorrentImportStage::FetchingManifest,
            TorrentImportStage::Validating,
            TorrentImportStage::Imported,
        ]
    );
    assert_eq!(finished.title.as_deref(), Some(release.title.as_str()));
    assert_eq!(
        finished.info_hash.as_deref(),
        Some(release.info_hash.as_str())
    );
    let course_id = finished.course_id.unwrap();
    {
        let library = library.lock().unwrap();
        let summary = library.courses().unwrap().remove(0);
        assert_eq!(
            summary.torrent_info_hash.as_deref(),
            Some(release.info_hash.as_str())
        );
        assert_eq!(
            library.torrent_info_hash(&course_id).unwrap().as_deref(),
            Some(release.info_hash.as_str())
        );
        let manifest = library.manifest(&course_id).unwrap().unwrap();
        let resource_id = manifest.modules[0].lessons[0].resources[0].id.clone();
        assert!(
            library
                .local_resource(&course_id, &resource_id)
                .unwrap()
                .is_none()
        );
        assert!(
            library
                .resource_path(&course_id, &resource_id)
                .unwrap()
                .is_none()
        );
    }
    let content = courses.join(&release.info_hash);
    assert_eq!(
        std::fs::read(content.join("course.json")).unwrap(),
        std::fs::read(release.package.join("course.json")).unwrap()
    );
    let info_hash = InfoHash::parse(&release.info_hash).unwrap();
    let stats = learner.transfer(&info_hash).unwrap();
    let largest = *stats.file_verified_bytes.iter().max().unwrap();
    assert!(
        largest < 1_500_000,
        "resource bytes were fetched: {stats:?}"
    );
    assert_lesson_streams(&learner, &library, &course_id, &release, &info_hash).await;
    learner.shutdown().await;

    let restarted = loopback_engine(&scratch.0, "learner-2", false).await;
    let restored = imports(
        Arc::clone(&restarted),
        &library,
        courses,
        TorrentImportTimeouts::default(),
    );
    assert_eq!(restored.restore().await, 1);
    assert!(restarted.transfer(&info_hash).is_some());
    restarted.shutdown().await;
    seeder.shutdown().await;
}

/// Uses the Local-network profile, so it multicasts local service discovery on
/// every interface of this machine. Opt in with `cargo test -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "announces on the local network; run manually with --ignored"]
async fn a_magnet_imports_through_local_service_discovery_alone() {
    let scratch = Scratch::new("lsd");
    let release = publish(&scratch.0);
    let local = |name: &str, allow_uploads: bool| EngineConfig {
        profile: NetworkProfile::LocalNetwork,
        state_directory: scratch.0.join(name),
        reachability: PeerReachability::default(),
        allow_uploads,
    };
    let seeder = Arc::new(LibrqbitEngine::start(local("seeder", true)).await.unwrap());
    seed(&seeder, release.torrent.clone(), release.package.clone()).await;
    // Let the seeder's own start-up announcement pass, as when a learner joins later.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let library = Arc::new(Mutex::new(
        CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
    ));
    let learner = Arc::new(
        LibrqbitEngine::start(local("learner", false))
            .await
            .unwrap(),
    );
    let importer = imports(
        Arc::clone(&learner),
        &library,
        scratch.0.join("courses"),
        TorrentImportTimeouts {
            metadata: Duration::from_secs(30),
            stall: Duration::from_secs(30),
            ..TorrentImportTimeouts::default()
        },
    );
    let magnet = format!("magnet:?xt=urn:btih:{}", release.info_hash);
    let started = Instant::now();
    let (finished, _) = run(&importer, TorrentSource::Magnet(magnet), vec![]).await;
    assert_eq!(finished.stage, TorrentImportStage::Imported, "{finished:?}");
    eprintln!(
        "imported through local discovery in {:?}",
        started.elapsed()
    );
    learner.shutdown().await;
    seeder.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrent_whose_inventory_differs_from_course_json_is_rejected() {
    let scratch = Scratch::new("mismatch");
    let release = publish(&scratch.0);
    let package = scratch.0.join("tampered");
    copy_tree(&release.package, &package);
    make_writable(&package);
    std::fs::write(package.join("extra.bin"), vec![1_u8; 1_000]).unwrap();
    let files = list_files(&package);
    let torrent = torrent_metainfo::build_torrent(
        &package,
        &files,
        &torrent_metainfo::TorrentOptions {
            name: "tampered".to_owned(),
            ..Default::default()
        },
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    let seeder = loopback_engine(&scratch.0, "seeder", true).await;
    let address = seed(&seeder, torrent.bytes.clone(), package.clone()).await;

    let library = Arc::new(Mutex::new(
        CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
    ));
    let learner = loopback_engine(&scratch.0, "learner", false).await;
    let courses = scratch.0.join("courses");
    let importer = imports(
        Arc::clone(&learner),
        &library,
        courses.clone(),
        TorrentImportTimeouts::default(),
    );
    let (finished, _) = run(
        &importer,
        TorrentSource::Metainfo(torrent.bytes),
        vec![address],
    )
    .await;
    assert_eq!(finished.stage, TorrentImportStage::Failed);
    assert_eq!(finished.failure.unwrap().code, "inventory_mismatch");
    assert_eq!(library.lock().unwrap().courses().unwrap().len(), 0);
    assert!(!courses.join(&torrent.info_hash).exists());
    assert_eq!(learner.transfers().len(), 0);
    learner.shutdown().await;
    seeder.shutdown().await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    NeverResolve,
    NoManifest,
    UnsafePath,
    StallManifest,
}

struct ScriptedEngine {
    script: Script,
    fetched: Mutex<Vec<FetchRequest>>,
}

#[allow(
    unknown_lints,
    clippy::unused_async_trait_impl,
    reason = "fakes mirror the async engine contract"
)]
impl TorrentEngine for ScriptedEngine {
    async fn seed(&self, _: SeedRequest) -> Result<InfoHash, EngineError> {
        Err(EngineError::UnsupportedCapability("seed"))
    }

    async fn resolve(&self, _: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        let file = |index, path: &str, length| TorrentFile {
            index,
            path: path.to_owned(),
            length,
        };
        let files = match self.script {
            Script::NeverResolve => return pending().await,
            Script::NoManifest => vec![file(0, "readme.txt", 10)],
            Script::UnsafePath => vec![file(0, "course.json", 10), file(1, "../escape", 10)],
            Script::StallManifest => vec![file(0, "course.json", 10), file(1, "a.bin", 10)],
        };
        Ok(ResolvedTorrent {
            info_hash: InfoHash::parse(&"c".repeat(40)).unwrap(),
            name: "scripted".to_owned(),
            files,
            metainfo: b"scripted".to_vec(),
            peers: Vec::new(),
        })
    }

    async fn fetch(&self, request: FetchRequest) -> Result<InfoHash, EngineError> {
        std::fs::create_dir_all(&request.content_root).unwrap();
        self.fetched.lock().unwrap().push(request);
        Ok(InfoHash::parse(&"c".repeat(40)).unwrap())
    }

    async fn stop(&self, _: &InfoHash) -> Result<(), EngineError> {
        self.fetched.lock().unwrap().clear();
        Ok(())
    }

    async fn open_file(
        &self,
        _: &InfoHash,
        _: usize,
    ) -> Result<Box<dyn torrent_port::VerifiedFile>, EngineError> {
        Err(EngineError::UnsupportedCapability("streaming"))
    }

    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats> {
        (!self.fetched.lock().unwrap().is_empty()).then(|| TransferStats {
            info_hash: info_hash.clone(),
            state: TransferState::Downloading,
            verified_bytes: 0,
            total_bytes: 20,
            uploaded_bytes: 0,
            upload_bytes_per_second: 0,
            downloaded_bytes: 0,
            connected_peers: 0,
            file_verified_bytes: vec![0, 0],
            failure: None,
        })
    }

    fn transfers(&self) -> Vec<TransferStats> {
        Vec::new()
    }

    fn listen_address(&self) -> Option<SocketAddr> {
        None
    }
}

fn scripted(script: Script) -> Arc<ScriptedEngine> {
    Arc::new(ScriptedEngine {
        script,
        fetched: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn unreachable_metadata_times_out_and_can_be_cancelled() {
    let scratch = Scratch::new("timeout");
    let library = Arc::new(Mutex::new(
        CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
    ));
    let importer = imports(
        scripted(Script::NeverResolve),
        &library,
        scratch.0.join("courses"),
        fast_timeouts(),
    );
    let source = || TorrentSource::Magnet(format!("magnet:?xt=urn:btih:{}", "c".repeat(40)));

    let (timed_out, _) = run(&importer, source(), vec![]).await;
    assert_eq!(timed_out.stage, TorrentImportStage::Failed);
    let failure = timed_out.failure.unwrap();
    assert_eq!(failure.code, "metadata_timeout");
    assert!(failure.message.contains("retry"));

    let job = importer.begin().unwrap();
    assert!(importer.begin().is_err(), "only one import may run");
    let job_id = job.job_id.clone();
    let mut ignore = |_: &TorrentImportSnapshot| {};
    let (cancelled, ()) = tokio::join!(
        importer.run(&job.job_id, source(), vec![], &mut ignore),
        async {
            tokio::time::sleep(Duration::from_millis(60)).await;
            assert!(importer.cancel(&job_id));
        }
    );
    assert_eq!(cancelled.unwrap().stage, TorrentImportStage::Cancelled);
    assert!(!importer.cancel(&job_id));
    assert!(importer.begin().is_ok());
}

#[tokio::test]
async fn non_courses_unsafe_inventories_and_stalled_manifests_fail_cleanly() {
    let scratch = Scratch::new("scripted");
    let library = Arc::new(Mutex::new(
        CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
    ));
    let courses = scratch.0.join("courses");
    let source = || TorrentSource::Metainfo(b"scripted".to_vec());
    for (script, code) in [
        (Script::NoManifest, "not_a_course"),
        (Script::UnsafePath, "unsafe_inventory"),
        (Script::StallManifest, "manifest_stalled"),
    ] {
        let engine = scripted(script);
        let importer = imports(
            Arc::clone(&engine),
            &library,
            courses.clone(),
            fast_timeouts(),
        );
        let (finished, _) = run(&importer, source(), vec![]).await;
        assert_eq!(finished.stage, TorrentImportStage::Failed);
        assert_eq!(finished.failure.unwrap().code, code);
        assert!(
            engine.fetched.lock().unwrap().is_empty(),
            "{code}: transfer not stopped"
        );
        assert!(
            !courses.join("c".repeat(40)).exists(),
            "{code}: cache not removed"
        );
    }
    assert_eq!(library.lock().unwrap().courses().unwrap().len(), 0);
}

fn list_files(root: &Path) -> Vec<torrent_metainfo::PackageFile> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(torrent_metainfo::PackageFile {
                    path: relative,
                    length: std::fs::metadata(&path).unwrap().len(),
                });
            }
        }
    }
    files
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

/// Bytes of `course.json` a fresh engine obtains from `peer`, waiting until
/// complete or `wait` elapses.
async fn manifest_bytes_from(
    root: &Path,
    name: &str,
    release: &Release,
    peer: SocketAddr,
    wait: Duration,
) -> u64 {
    let probe = loopback_engine(root, name, false).await;
    let info_hash = probe
        .fetch(FetchRequest {
            metainfo: release.torrent.clone(),
            content_root: root.join(format!("{name}-content")),
            files: vec![0],
            initial_peers: vec![peer],
        })
        .await
        .unwrap();
    let length = std::fs::metadata(release.package.join("course.json"))
        .unwrap()
        .len();
    let deadline = Instant::now() + wait;
    let mut verified = 0;
    while Instant::now() < deadline {
        verified = probe
            .transfer(&info_hash)
            .and_then(|stats| stats.file_verified_bytes.first().copied())
            .unwrap_or(0);
        if verified == length {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    probe.shutdown().await;
    verified
}

struct Learner {
    private: Arc<LibrqbitEngine>,
    sharing: Arc<LibrqbitEngine>,
    router: Arc<TransferRouter<LibrqbitEngine>>,
    courses: CourseSharing<LibrqbitEngine>,
}

async fn learner(root: &Path, library: &Arc<Mutex<CourseLibrary>>, generation: u8) -> Learner {
    let private = loopback_engine(root, &format!("private-{generation}"), false).await;
    let sharing = loopback_engine(root, &format!("sharing-{generation}"), true).await;
    let router = Arc::new(TransferRouter::new(
        Arc::clone(&private),
        Some(Arc::clone(&sharing)),
    ));
    let courses = CourseSharing::new(Arc::clone(&router), Arc::clone(library));
    courses.load().unwrap();
    Learner {
        private,
        sharing,
        router,
        courses,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_courses_the_learner_shares_are_served_and_the_choice_survives_restart() {
    let scratch = Scratch::new("sharing");
    let release = publish(&scratch.0);
    let seeder = loopback_engine(&scratch.0, "seeder", true).await;
    let seeder_address = seed(&seeder, release.torrent.clone(), release.package.clone()).await;
    let library = Arc::new(Mutex::new(
        CourseLibrary::open(&scratch.0.join("library.sqlite3"), ImportLimits::default()).unwrap(),
    ));
    let first = learner(&scratch.0, &library, 1).await;
    let importer = imports(
        Arc::clone(&first.router),
        &library,
        scratch.0.join("courses"),
        TorrentImportTimeouts::default(),
    );
    let magnet = format!("magnet:?xt=urn:btih:{}", release.info_hash);
    let (finished, _) = run(
        &importer,
        TorrentSource::Magnet(magnet),
        vec![seeder_address],
    )
    .await;
    let course_id = finished.course_id.expect("imported");
    let info_hash = InfoHash::parse(&release.info_hash).unwrap();
    seeder.shutdown().await;

    // Imported courses stay private: nothing is served.
    assert!(first.private.transfer(&info_hash).is_some());
    assert!(first.sharing.transfer(&info_hash).is_none());
    let private_address = first.private.listen_address().unwrap();
    let served = manifest_bytes_from(
        &scratch.0,
        "probe-private",
        &release,
        private_address,
        Duration::from_secs(4),
    )
    .await;
    assert_eq!(served, 0);

    // Sharing moves the transfer and re-checks course.json from disk (the seeder is gone).
    first.courses.set(&course_id, true).await.unwrap();
    assert!(first.private.transfer(&info_hash).is_none());
    assert!(first.sharing.transfer(&info_hash).is_some());
    let sharing_address = first.sharing.listen_address().unwrap();
    let served = manifest_bytes_from(
        &scratch.0,
        "probe-sharing",
        &release,
        sharing_address,
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(
        served,
        std::fs::metadata(release.package.join("course.json"))
            .unwrap()
            .len()
    );
    let status = first.courses.statuses().unwrap().remove(0);
    assert!(status.sharing);
    assert!(status.transfer.unwrap().uploaded_bytes >= served);
    assert!(library.lock().unwrap().courses().unwrap()[0].sharing);
    first.private.shutdown().await;
    first.sharing.shutdown().await;

    // A restart restores the transfer into the sharing engine.
    let second = learner(&scratch.0, &library, 2).await;
    let restorer = imports(
        Arc::clone(&second.router),
        &library,
        scratch.0.join("courses"),
        TorrentImportTimeouts::default(),
    );
    assert_eq!(restorer.restore().await, 1);
    assert!(second.sharing.transfer(&info_hash).is_some());
    assert!(second.private.transfer(&info_hash).is_none());

    // Stopping sharing moves it back and is stored.
    second.courses.set(&course_id, false).await.unwrap();
    assert!(second.private.transfer(&info_hash).is_some());
    assert!(second.sharing.transfer(&info_hash).is_none());
    assert!(!library.lock().unwrap().courses().unwrap()[0].sharing);
    assert!(matches!(
        second.courses.set("course_unknown", true).await,
        Err(SharingError::NotTorrentCourse)
    ));
    second.private.shutdown().await;
    second.sharing.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sharing_is_refused_without_a_sharing_engine() {
    let scratch = Scratch::new("no-sharing-engine");
    let library = Arc::new(Mutex::new(
        CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
    ));
    let private = loopback_engine(&scratch.0, "private", false).await;
    let router = Arc::new(TransferRouter::new(Arc::clone(&private), None));
    let courses = CourseSharing::new(Arc::clone(&router), Arc::clone(&library));
    assert!(!courses.available());
    let info_hash = InfoHash::parse(&"d".repeat(40)).unwrap();
    assert!(matches!(
        router.set_shared(&info_hash, true).await,
        Err(EngineError::UnsupportedCapability(_))
    ));
    router.set_shared(&info_hash, false).await.unwrap();
    private.shutdown().await;
}

struct Imported {
    scratch: Scratch,
    seeder: Arc<LibrqbitEngine>,
    learner: Arc<LibrqbitEngine>,
    library: Arc<Mutex<CourseLibrary>>,
    courses: PathBuf,
    course_id: String,
    info_hash: InfoHash,
}

/// A course imported by magnet from a loopback seeder into a file-backed library.
async fn imported_course(name: &str) -> Imported {
    let scratch = Scratch::new(name);
    let release = publish(&scratch.0);
    let seeder = loopback_engine(&scratch.0, "seeder", true).await;
    let address = seed(&seeder, release.torrent.clone(), release.package.clone()).await;
    let library = Arc::new(Mutex::new(
        CourseLibrary::open(&scratch.0.join("library.sqlite3"), ImportLimits::default()).unwrap(),
    ));
    let learner = loopback_engine(&scratch.0, "learner", false).await;
    let courses = scratch.0.join("courses");
    let importer = imports(
        Arc::clone(&learner),
        &library,
        courses.clone(),
        TorrentImportTimeouts::default(),
    );
    let magnet = format!("magnet:?xt=urn:btih:{}", release.info_hash);
    let (finished, _) = run(&importer, TorrentSource::Magnet(magnet), vec![address]).await;
    Imported {
        course_id: finished.course_id.expect("imported"),
        info_hash: InfoHash::parse(&release.info_hash).unwrap(),
        scratch,
        seeder,
        learner,
        library,
        courses,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn kept_lessons_download_in_full_and_are_selected_again_on_restore() {
    use learning_application::{Availability, DownloadScope};

    let course = imported_course("downloads").await;
    let library = &course.library;
    let course_id = &course.course_id;
    let (lesson_id, resource) = {
        let library = library.lock().unwrap();
        let manifest = library.manifest(course_id).unwrap().unwrap();
        let resource = library.torrent_resources(course_id).unwrap().remove(0);
        (manifest.modules[0].lessons[0].id.clone(), resource)
    };
    let manifest_only = library
        .lock()
        .unwrap()
        .download_selection(course_id)
        .unwrap()
        .unwrap();
    assert_eq!(manifest_only.len(), 1);
    {
        let mut library = library.lock().unwrap();
        assert!(
            !library
                .set_download(
                    course_id,
                    &DownloadScope::Lesson("lesson_x".to_owned()),
                    true
                )
                .unwrap()
        );
        assert!(
            !library
                .set_download(
                    course_id,
                    &DownloadScope::Module("module_x".to_owned()),
                    true
                )
                .unwrap()
        );
        assert!(
            !library
                .set_download("course_unknown", &DownloadScope::Course, true)
                .unwrap()
        );
        assert!(
            library
                .set_download(course_id, &DownloadScope::Lesson(lesson_id), true)
                .unwrap()
        );
        assert!(
            library
                .committed_resources(course_id)
                .unwrap()
                .contains(&resource.resource_id)
        );
    }
    let selection = library
        .lock()
        .unwrap()
        .download_selection(course_id)
        .unwrap()
        .unwrap();
    assert_eq!(selection, [manifest_only[0], resource.file_index]);
    let verified = |engine: &LibrqbitEngine| {
        engine
            .transfer(&course.info_hash)
            .unwrap()
            .file_verified_bytes[resource.file_index]
    };
    assert_eq!(
        Availability::of(true, verified(&course.learner), resource.byte_length),
        Availability::Downloading
    );

    course
        .learner
        .select_files(&course.info_hash, &selection)
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while verified(&course.learner) < resource.byte_length {
        assert!(Instant::now() < deadline, "the kept lesson downloads");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        Availability::of(true, verified(&course.learner), resource.byte_length),
        Availability::AvailableOffline
    );
    course.learner.shutdown().await;
    course.seeder.shutdown().await;

    // A restore selects the kept lesson again, not just the manifest.
    let restoring = scripted(Script::StallManifest);
    let restorer = imports(
        Arc::clone(&restoring),
        library,
        course.courses.clone(),
        TorrentImportTimeouts::default(),
    );
    assert_eq!(restorer.restore().await, 1);
    assert_eq!(restoring.fetched.lock().unwrap()[0].files, selection);
    drop(course.scratch);
}

#[tokio::test(flavor = "multi_thread")]
async fn module_and_course_scopes_cover_their_lessons_and_labels_follow_availability() {
    use learning_application::{Availability, DownloadScope};

    let course = imported_course("download-scopes").await;
    let course_id = &course.course_id;
    {
        let mut library = course.library.lock().unwrap();
        let module_id = library.manifest(course_id).unwrap().unwrap().modules[0]
            .id
            .clone();
        let resource = library.torrent_resources(course_id).unwrap().remove(0);
        let manifest_only = library.download_selection(course_id).unwrap().unwrap();
        let with_lesson = [manifest_only[0], resource.file_index];

        library
            .set_download(course_id, &DownloadScope::Module(module_id.clone()), true)
            .unwrap();
        assert_eq!(
            library.download_selection(course_id).unwrap().unwrap(),
            with_lesson
        );
        library
            .set_download(course_id, &DownloadScope::Module(module_id), false)
            .unwrap();
        assert_eq!(
            library.download_selection(course_id).unwrap().unwrap(),
            manifest_only
        );
        library
            .set_download(course_id, &DownloadScope::Course, true)
            .unwrap();
        assert_eq!(
            library.downloads(course_id).unwrap(),
            [DownloadScope::Course]
        );
        assert_eq!(
            library.download_selection(course_id).unwrap().unwrap(),
            with_lesson
        );
    }

    assert_eq!(Availability::of(false, 0, 10), Availability::OnlineOnly);
    assert_eq!(
        Availability::of(false, 4, 10),
        Availability::PartiallyAvailable
    );
    assert_eq!(Availability::of(true, 4, 10), Availability::Downloading);
    assert_eq!(
        Availability::of(false, 10, 10).as_str(),
        "available_offline"
    );
    course.learner.shutdown().await;
    course.seeder.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn paused_courses_are_restored_paused() {
    let course = imported_course("paused").await;
    {
        let mut library = course.library.lock().unwrap();
        assert!(!library.course_paused(&course.course_id).unwrap());
        assert!(!library.set_course_paused("course_unknown", true).unwrap());
        assert!(library.set_course_paused(&course.course_id, true).unwrap());
        assert!(library.course_paused(&course.course_id).unwrap());
    }
    course.learner.shutdown().await;

    let restarted = loopback_engine(&course.scratch.0, "learner-2", false).await;
    let restorer = imports(
        Arc::clone(&restarted),
        &course.library,
        course.courses.clone(),
        TorrentImportTimeouts::default(),
    );
    assert_eq!(restorer.restore().await, 1);
    assert_eq!(
        restarted.transfer(&course.info_hash).unwrap().state,
        TransferState::Paused
    );
    course
        .library
        .lock()
        .unwrap()
        .set_course_paused(&course.course_id, false)
        .unwrap();
    restarted.shutdown().await;
    course.seeder.shutdown().await;
}
