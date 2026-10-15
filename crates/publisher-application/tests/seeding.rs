//! Release seeding orchestration against a scripted engine.

use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use publisher_application::{
    DistributionOptions, DraftServiceError, EngineError, MetadataEdit, NetworkProfile, PublishJobs,
    PublisherDrafts, SeedingService, StructureEdit, TransferState, TransferStats,
};
use torrent_port::{
    FetchRequest, InfoHash, ResolveRequest, ResolvedTorrent, SeedRequest, TorrentEngine,
};

#[derive(Default)]
struct FakeEngine {
    active: Mutex<HashMap<InfoHash, SeedRequest>>,
    refuse: Mutex<Option<EngineError>>,
}

impl TorrentEngine for FakeEngine {
    async fn seed(&self, request: SeedRequest) -> Result<InfoHash, EngineError> {
        if let Some(error) = self.refuse.lock().unwrap().clone() {
            return Err(error);
        }
        let parsed = torrent_metainfo_hash(&request.torrent);
        let mut active = self.active.lock().unwrap();
        if active.contains_key(&parsed) {
            return Err(EngineError::AlreadyActive);
        }
        active.insert(parsed.clone(), request);
        Ok(parsed)
    }

    async fn resolve(&self, _: ResolveRequest) -> Result<ResolvedTorrent, EngineError> {
        Err(EngineError::UnsupportedCapability("resolve"))
    }

    async fn fetch(&self, _: FetchRequest) -> Result<InfoHash, EngineError> {
        Err(EngineError::UnsupportedCapability("fetch"))
    }

    async fn stop(&self, info_hash: &InfoHash) -> Result<(), EngineError> {
        self.active
            .lock()
            .unwrap()
            .remove(info_hash)
            .map(|_| ())
            .ok_or(EngineError::NotActive)
    }

    fn transfer(&self, info_hash: &InfoHash) -> Option<TransferStats> {
        self.active
            .lock()
            .unwrap()
            .contains_key(info_hash)
            .then(|| TransferStats {
                info_hash: info_hash.clone(),
                state: TransferState::Seeding,
                verified_bytes: 10,
                total_bytes: 10,
                uploaded_bytes: 5,
                upload_bytes_per_second: 1,
                downloaded_bytes: 0,
                connected_peers: 1,
                file_verified_bytes: vec![10],
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

fn torrent_metainfo_hash(torrent: &[u8]) -> InfoHash {
    use sha1::{Digest, Sha1};
    let top = torrent_metainfo::decode_dict_with_spans(torrent).unwrap();
    let span = top[b"info".as_slice()].1.clone();
    let hex = Sha1::digest(&torrent[span])
        .iter()
        .fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    InfoHash::parse(&hex).unwrap()
}

struct Fixture {
    root: PathBuf,
    drafts: Arc<Mutex<PublisherDrafts>>,
    engine: Arc<FakeEngine>,
    service: SeedingService<FakeEngine>,
    release_ids: Vec<String>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        make_writable(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str, versions: &[&str]) -> Fixture {
    let root =
        std::env::temp_dir().join(format!("zeron-app-seeding-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let database = root.join("publisher.sqlite3");
    let mut drafts = PublisherDrafts::open(&database).unwrap();
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
    let source = root.join("notes.pdf");
    fs::write(&source, vec![1_u8; 30_000]).unwrap();
    drafts
        .attach_source(&draft_id, 2, &lesson_id, &source)
        .unwrap();
    let jobs = PublishJobs::new(&root.join("staging"));
    let mut release_ids = Vec::new();
    for version in versions {
        let revision = drafts.draft(&draft_id).unwrap().unwrap().draft.revision;
        drafts
            .update_metadata(
                &draft_id,
                revision,
                MetadataEdit {
                    version: (*version).to_owned(),
                    title: "Seeding".to_owned(),
                    publisher_name: "Zeron Tests".to_owned(),
                    language: "en".to_owned(),
                    ..MetadataEdit::default()
                },
            )
            .unwrap();
        let draft = drafts.draft(&draft_id).unwrap().unwrap().draft;
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
        release_ids.push(record.release.release_id);
    }
    let drafts = Arc::new(Mutex::new(drafts));
    let engine = Arc::new(FakeEngine::default());
    let service = SeedingService::new(
        Arc::clone(&drafts),
        Arc::clone(&engine),
        root.join("releases"),
    );
    Fixture {
        root,
        drafts,
        engine,
        service,
        release_ids,
    }
}

#[tokio::test]
async fn starting_persists_intent_and_restore_resumes_it() {
    let fixture = fixture("restore", &["1.0.0", "1.1.0"]);
    let first = fixture.release_ids[0].clone();
    fixture.service.start(&first).await.unwrap();
    fixture.service.start(&first).await.unwrap();
    {
        let active = fixture.engine.active.lock().unwrap();
        assert_eq!(active.len(), 1);
        let request = active.values().next().unwrap();
        assert!(request.content_root.ends_with(format!("{first}/package")));
    }

    let statuses = fixture.service.statuses().unwrap();
    assert_eq!(statuses.len(), 2);
    let status = statuses
        .iter()
        .find(|status| status.release.release.release_id == first)
        .unwrap();
    assert!(status.enabled);
    assert_eq!(
        status.transfer.as_ref().unwrap().state,
        TransferState::Seeding
    );
    let other = statuses
        .iter()
        .find(|status| status.release.release.release_id != first)
        .unwrap();
    assert!(!other.enabled && other.transfer.is_none());

    let restarted_engine = Arc::new(FakeEngine::default());
    let restarted = SeedingService::new(
        Arc::clone(&fixture.drafts),
        Arc::clone(&restarted_engine),
        fixture.root.join("releases"),
    );
    assert_eq!(restarted.restore().await.unwrap(), 1);
    assert_eq!(restarted_engine.active.lock().unwrap().len(), 1);

    restarted.stop(&first).await.unwrap();
    restarted.stop(&first).await.unwrap();
    assert!(restarted_engine.active.lock().unwrap().is_empty());
    assert!(
        restarted
            .statuses()
            .unwrap()
            .iter()
            .all(|status| !status.enabled)
    );
    assert_eq!(restarted.restore().await.unwrap(), 0);
}

#[tokio::test]
async fn failures_are_reported_without_recording_intent() {
    let fixture = fixture("failures", &["1.0.0"]);
    let release_id = fixture.release_ids[0].clone();
    *fixture.engine.refuse.lock().unwrap() = Some(EngineError::Engine("no sockets".to_owned()));
    assert!(matches!(
        fixture.service.start(&release_id).await,
        Err(DraftServiceError::Seeding(EngineError::Engine(_)))
    ));
    let status = fixture.service.statuses().unwrap().remove(0);
    assert!(!status.enabled);
    assert!(status.start_error.unwrap().contains("no sockets"));

    *fixture.engine.refuse.lock().unwrap() = None;
    fixture.service.start(&release_id).await.unwrap();
    assert!(fixture.service.statuses().unwrap()[0].start_error.is_none());
    fixture.service.stop(&release_id).await.unwrap();

    let package = fixture
        .root
        .join("releases")
        .join(&release_id)
        .join("package");
    make_writable(&package);
    fs::write(package.join("unexpected.txt"), b"x").unwrap();
    assert!(matches!(
        fixture.service.start(&release_id).await,
        Err(DraftServiceError::InvalidAttempt(_))
    ));
    assert!(fixture.engine.active.lock().unwrap().is_empty());
    assert!(matches!(
        fixture
            .service
            .start("release_0123456789abcdef0123456789abcdef")
            .await,
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}

#[test]
fn the_network_profile_defaults_to_the_local_network() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert_eq!(
        drafts.network_profile().unwrap(),
        NetworkProfile::LocalNetwork
    );
    drafts
        .set_network_profile(NetworkProfile::Internet)
        .unwrap();
    assert_eq!(drafts.network_profile().unwrap(), NetworkProfile::Internet);
}

fn make_writable(path: &std::path::Path) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            make_writable(&path);
        } else if let Ok(metadata) = fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[expect(clippy::permissions_set_readonly_false, reason = "test setup")]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&path, permissions);
        }
    }
}
