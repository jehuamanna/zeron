//! The Library verifies publisher signatures and pins each course to its
//! publisher on first use (ADR 0014).

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use course_signing::{RELEASE_SIGNATURE_FILE, revoke};
use learning_application::{
    CancellationFlag, CourseLibrary, ImportLimits, LibraryError, TorrentImportStage,
    TorrentImportTimeouts, TorrentImports, TorrentSource,
};
use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, ReleaseSigner, StructureEdit,
};
use publisher_identity::{Identity, MemoryStore, create, signing_key};
use torrent_librqbit::{EngineConfig, LibrqbitEngine, PeerReachability};
use torrent_port::{NetworkProfile, SeedRequest, TorrentEngine, TransferState};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-signed-import-{name}-{}", std::process::id()));
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

/// A publisher database with one course; every release of it shares the
/// course ID, whoever signs it.
struct Publisher {
    root: PathBuf,
    drafts: PublisherDrafts,
    jobs: PublishJobs,
    draft_id: String,
    revision: u64,
}

struct Release {
    package: PathBuf,
    torrent: Vec<u8>,
    info_hash: String,
}

impl Publisher {
    fn new(root: &Path) -> Self {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = drafts.create().unwrap().draft.draft_id;
        let module_id = drafts
            .edit_structure(
                &draft_id,
                0,
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
                1,
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
        std::fs::write(&source, vec![7_u8; 200_000]).unwrap();
        drafts
            .attach_source(&draft_id, 2, &lesson_id, &source)
            .unwrap();
        Self {
            root: root.to_path_buf(),
            drafts,
            jobs: PublishJobs::new(&root.join("staging")),
            draft_id,
            revision: 3,
        }
    }

    fn release(
        &mut self,
        version: &str,
        signer: Option<(&Identity, &MemoryStore, u64)>,
    ) -> Release {
        self.drafts
            .update_metadata(
                &self.draft_id,
                self.revision,
                MetadataEdit {
                    version: version.to_owned(),
                    title: "Signed Course".to_owned(),
                    publisher_name: "Zeron Tests".to_owned(),
                    language: "en".to_owned(),
                    ..MetadataEdit::default()
                },
            )
            .unwrap();
        self.revision += 1;
        let draft = self.drafts.draft(&self.draft_id).unwrap().unwrap().draft;
        let signer = signer.map(|(identity, store, at)| {
            ReleaseSigner::new(identity.clone(), signing_key(store, identity).unwrap(), at).unwrap()
        });
        let job = self.jobs.begin(&draft).unwrap();
        self.jobs
            .run_signed(
                &job.job_id,
                &draft,
                &DistributionOptions::default(),
                signer.as_ref(),
                &mut |_| {},
            )
            .unwrap();
        let releases = self.root.join("releases");
        let (record, _) = self
            .jobs
            .activate(&job.job_id, &mut self.drafts, &releases)
            .unwrap();
        let directory = releases.join(&record.release.release_id);
        Release {
            package: directory.join("package"),
            torrent: std::fs::read(directory.join("release.torrent")).unwrap(),
            info_hash: record.release.info_hash,
        }
    }
}

fn library(root: &Path) -> CourseLibrary {
    CourseLibrary::open(&root.join("library.sqlite3"), ImportLimits::default()).unwrap()
}

fn import(library: &mut CourseLibrary, release: &Release) -> Result<String, LibraryError> {
    library
        .import_local_folder(&release.package, &CancellationFlag::default())
        .map(|receipt| receipt.course_version)
}

#[test]
fn a_signed_course_pins_its_publisher_and_refuses_impostors_and_unsigned_updates() {
    let scratch = Scratch::new("folder");
    let mut publisher = Publisher::new(&scratch.0);
    let (store, impostor_store) = (MemoryStore::default(), MemoryStore::default());
    let identity = create(&store, 1).unwrap();
    let impostor = create(&impostor_store, 1).unwrap();
    let mut library = library(&scratch.0);

    let first = publisher.release("1.0.0", Some((&identity, &store, 10)));
    assert_eq!(import(&mut library, &first).unwrap(), "1.0.0");
    let course_id = library.courses().unwrap()[0].course_id.clone();
    let status = library.course_publisher(&course_id).unwrap();
    assert_eq!(status.pinned.as_deref(), Some(identity.root.as_str()));
    assert_eq!(status.signature.as_ref().unwrap().signed_at, 10);
    assert_eq!(status.fingerprint(), identity.fingerprint().ok());

    let forged = publisher.release("1.1.0", Some((&impostor, &impostor_store, 20)));
    match import(&mut library, &forged) {
        Err(LibraryError::PublisherChanged { pinned, found }) => {
            assert_eq!(pinned, identity.fingerprint().unwrap());
            assert_eq!(found, impostor.fingerprint().ok());
        }
        other => panic!("expected a publisher change, got {other:?}"),
    }
    let unsigned = publisher.release("1.2.0", None);
    assert!(matches!(
        import(&mut library, &unsigned),
        Err(LibraryError::PublisherChanged { found: None, .. })
    ));
    assert_eq!(
        library.manifest(&course_id).unwrap().unwrap().version,
        "1.0.0"
    );

    let update = publisher.release("1.3.0", Some((&identity, &store, 30)));
    assert_eq!(import(&mut library, &update).unwrap(), "1.3.0");
    assert_eq!(
        library
            .course_publisher(&course_id)
            .unwrap()
            .signature
            .unwrap()
            .signed_at,
        30
    );

    assert!(library.forget_course_publisher(&course_id).unwrap());
    assert_eq!(import(&mut library, &forged).unwrap(), "1.1.0");
    assert_eq!(
        library
            .course_publisher(&course_id)
            .unwrap()
            .pinned
            .as_deref(),
        Some(impostor.root.as_str())
    );
}

#[test]
fn changed_signatures_and_revoked_keys_are_refused() {
    let scratch = Scratch::new("tamper");
    let mut publisher = Publisher::new(&scratch.0);
    let store = MemoryStore::default();
    let identity = create(&store, 1).unwrap();
    let release = publisher.release("1.0.0", Some((&identity, &store, 100)));

    let copy = scratch.0.join("copy");
    copy_tree(&release.package, &copy);
    make_writable(&copy);
    let signature_path = copy.join(RELEASE_SIGNATURE_FILE);
    let text = std::fs::read_to_string(&signature_path).unwrap();
    std::fs::write(
        &signature_path,
        text.replace("\"signedAt\": 100", "\"signedAt\": 101"),
    )
    .unwrap();
    let mut library = library(&scratch.0);
    assert!(matches!(
        library.import_local_folder(&copy, &CancellationFlag::default()),
        Err(LibraryError::Signature(_))
    ));
    assert_eq!(library.courses().unwrap().len(), 0);

    let secret = signing_key(&store, &identity).unwrap();
    let revocation = revoke(
        &secret,
        &identity.current_key().unwrap(),
        50,
        "laptop stolen",
    );
    library.add_key_revocation(&revocation).unwrap();
    match import(&mut library, &release) {
        Err(LibraryError::Signature(message)) => {
            assert!(message.contains("laptop stolen"), "{message}");
        }
        other => panic!("expected a revoked signature, got {other:?}"),
    }
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

async fn seed(engine: &LibrqbitEngine, release: &Release) -> SocketAddr {
    let info_hash = engine
        .seed(SeedRequest {
            torrent: release.torrent.clone(),
            content_root: release.package.clone(),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn magnet_imports_fetch_and_verify_the_signature_and_refuse_an_impostor() {
    let scratch = Scratch::new("magnet");
    let mut publisher = Publisher::new(&scratch.0);
    let (store, impostor_store) = (MemoryStore::default(), MemoryStore::default());
    let identity = create(&store, 1).unwrap();
    let impostor = create(&impostor_store, 1).unwrap();
    let genuine = publisher.release("1.0.0", Some((&identity, &store, 10)));
    let forged = publisher.release("1.1.0", Some((&impostor, &impostor_store, 20)));

    let seeder = engine(&scratch.0, "seeder", true).await;
    let genuine_peer = seed(&seeder, &genuine).await;
    let forged_peer = seed(&seeder, &forged).await;
    let library = Arc::new(Mutex::new(library(&scratch.0)));
    let learner = engine(&scratch.0, "learner", false).await;
    let imports = TorrentImports::new(
        Arc::clone(&learner),
        Arc::clone(&library),
        scratch.0.join("courses"),
        ImportLimits::default(),
        TorrentImportTimeouts::default(),
    );
    let run = |info_hash: String, peer: SocketAddr| {
        let imports = &imports;
        async move {
            let job = imports.begin().unwrap();
            imports
                .run(
                    &job.job_id,
                    TorrentSource::Magnet(format!("magnet:?xt=urn:btih:{info_hash}")),
                    vec![peer],
                    &mut |_| {},
                )
                .await
                .unwrap()
        }
    };

    let imported = run(genuine.info_hash.clone(), genuine_peer).await;
    assert_eq!(imported.stage, TorrentImportStage::Imported, "{imported:?}");
    let course_id = imported.course_id.unwrap();
    {
        let library = library.lock().unwrap();
        let status = library.course_publisher(&course_id).unwrap();
        assert_eq!(status.pinned.as_deref(), Some(identity.root.as_str()));
        let selection = library.download_selection(&course_id).unwrap().unwrap();
        assert_eq!(selection.len(), 2, "course.json and the signature are kept");
    }

    let refused = run(forged.info_hash.clone(), forged_peer).await;
    assert_eq!(refused.stage, TorrentImportStage::Failed, "{refused:?}");
    let failure = refused.failure.unwrap();
    assert_eq!(failure.code, "publisher_changed");
    assert!(failure.message.contains(&identity.fingerprint().unwrap()));
    assert_eq!(
        library
            .lock()
            .unwrap()
            .manifest(&course_id)
            .unwrap()
            .unwrap()
            .version,
        "1.0.0"
    );
}
