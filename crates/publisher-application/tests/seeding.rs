//! Release seeding orchestration against a scripted engine.

use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use publisher_application::{
    DEFAULT_TRACKER_LISTS, DistributionOptions, DraftServiceError, EngineError, MAX_TRACKERS,
    MetadataEdit, NetworkProfile, PeerReachability, PublishJobs, PublisherDrafts, ReleaseStatus,
    SeedingService, StructureEdit, TrackerListStatus, TransferState, TransferStats,
    merge_tracker_lists,
};
use torrent_port::{
    FetchRequest, InfoHash, ResolveRequest, ResolvedTorrent, SeedRequest, TorrentEngine,
};

#[derive(Default)]
struct FakeEngine {
    active: Mutex<HashMap<InfoHash, SeedRequest>>,
    refuse: Mutex<Option<EngineError>>,
}

#[allow(
    unknown_lints,
    clippy::unused_async_trait_impl,
    reason = "fakes mirror the async engine contract"
)]
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
        if let Some(error) = self.refuse.lock().unwrap().clone() {
            return Err(error);
        }
        self.active
            .lock()
            .unwrap()
            .remove(info_hash)
            .map(|_| ())
            .ok_or(EngineError::NotActive)
    }

    async fn open_file(
        &self,
        _: &InfoHash,
        _: usize,
    ) -> Result<Box<dyn torrent_port::VerifiedFile>, EngineError> {
        Err(EngineError::UnsupportedCapability("streaming"))
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

#[tokio::test]
async fn archiving_stops_seeding_keeps_files_and_refuses_restarts() {
    let fixture = fixture("archive", &["1.0.0", "1.1.0"]);
    let (older, newer) = (
        fixture.release_ids[0].clone(),
        fixture.release_ids[1].clone(),
    );
    fixture.service.start(&older).await.unwrap();
    fixture.service.start(&newer).await.unwrap();

    *fixture.engine.refuse.lock().unwrap() = Some(EngineError::Engine("stuck".to_owned()));
    assert!(matches!(
        fixture.service.archive(&newer).await,
        Err(DraftServiceError::Seeding(_))
    ));
    assert_eq!(
        fixture
            .drafts
            .lock()
            .unwrap()
            .release(&newer)
            .unwrap()
            .unwrap()
            .status,
        ReleaseStatus::Published
    );
    *fixture.engine.refuse.lock().unwrap() = None;

    let archived = fixture.service.archive(&newer).await.unwrap();
    assert_eq!(archived.status, ReleaseStatus::Archived);
    assert_eq!(fixture.engine.active.lock().unwrap().len(), 1);
    let statuses = fixture.service.statuses().unwrap();
    let status = |id: &str| {
        statuses
            .iter()
            .find(|status| status.release.release.release_id == id)
            .unwrap()
            .clone()
    };
    assert!(!status(&newer).enabled && status(&newer).transfer.is_none());
    assert!(status(&older).enabled);
    assert_eq!(status(&older).release.status, ReleaseStatus::Superseded);

    let package = fixture.root.join("releases").join(&newer).join("package");
    assert!(package.join("course.json").is_file());
    assert!(
        fs::metadata(package.join("course.json"))
            .unwrap()
            .permissions()
            .readonly()
    );

    assert!(matches!(
        fixture.service.start(&newer).await,
        Err(DraftServiceError::ReleaseArchived)
    ));
    assert!(matches!(
        fixture
            .drafts
            .lock()
            .unwrap()
            .seed_request(&fixture.root.join("releases"), &newer),
        Err(DraftServiceError::ReleaseArchived)
    ));
    assert_eq!(fixture.service.archive(&newer).await.unwrap(), archived);

    let restarted_engine = Arc::new(FakeEngine::default());
    let restarted = SeedingService::new(
        Arc::clone(&fixture.drafts),
        Arc::clone(&restarted_engine),
        fixture.root.join("releases"),
    );
    assert_eq!(restarted.restore().await.unwrap(), 1);
    assert_eq!(restarted_engine.active.lock().unwrap().len(), 1);
    assert!(matches!(
        restarted
            .archive("release_0123456789abcdef0123456789abcdef")
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

    assert_eq!(
        drafts.peer_reachability().unwrap(),
        PeerReachability::default()
    );
    let chosen = PeerReachability::new(Some(51413), true).unwrap();
    drafts.set_peer_reachability(chosen).unwrap();
    assert_eq!(drafts.peer_reachability().unwrap(), chosen);
    drafts
        .set_peer_reachability(PeerReachability::default())
        .unwrap();
    assert_eq!(
        drafts.peer_reachability().unwrap(),
        PeerReachability::default()
    );
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

#[test]
fn trackers_are_validated_stored_in_order_and_become_one_tier_each() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert_eq!(drafts.trackers().unwrap().len(), 0);
    assert_eq!(
        drafts.distribution_options().unwrap(),
        DistributionOptions::default()
    );

    let chosen = drafts
        .set_trackers(&[
            " udp://tracker.example:6969/announce ".to_owned(),
            String::new(),
            "https://tracker.example/announce".to_owned(),
            "udp://tracker.example:6969/announce".to_owned(),
        ])
        .unwrap();
    let expected = vec![
        "udp://tracker.example:6969/announce".to_owned(),
        "https://tracker.example/announce".to_owned(),
    ];
    assert_eq!(chosen, expected);
    assert_eq!(drafts.trackers().unwrap(), expected);
    assert_eq!(
        drafts.distribution_options().unwrap().tracker_tiers,
        vec![vec![expected[0].clone()], vec![expected[1].clone()]]
    );

    for bad in [
        vec!["ftp://tracker.example/announce".to_owned()],
        vec!["udp://".to_owned()],
        vec!["tracker.example:6969".to_owned()],
        vec![format!("https://tracker.example/{}", "a".repeat(600))],
        vec!["https://tracker.example/a b".to_owned()],
        (0..101)
            .map(|index| format!("udp://t{index}.example:6969/announce"))
            .collect(),
        (0..70)
            .map(|index| {
                format!(
                    "udp://tracker-{index:03}.{}.example:6969/announce",
                    "a".repeat(40)
                )
            })
            .collect(),
    ] {
        let error = drafts.set_trackers(&bad).unwrap_err();
        assert!(
            matches!(&error, DraftServiceError::InvalidInput { field, .. } if field == "trackers"),
            "{bad:?}: {error}"
        );
    }
    assert_eq!(
        drafts.trackers().unwrap(),
        expected,
        "rejected input changes nothing"
    );

    drafts.set_trackers(&[]).unwrap();
    assert_eq!(drafts.trackers().unwrap().len(), 0);
}

#[test]
fn published_tracker_lists_merge_in_order_within_the_limits() {
    let first =
        "udp://tracker.one.example:1337/announce\n\nhttp://tracker.two.example:80/announce\n";
    let second = "# comment\nwss://tracker.webtorrent.example\nudp://tracker.one.example:1337/announce\nnot a url\nhttps://tracker.three.example:443/announce";
    assert_eq!(
        merge_tracker_lists(&[first, second]),
        vec![
            "udp://tracker.one.example:1337/announce".to_owned(),
            "http://tracker.two.example:80/announce".to_owned(),
            "https://tracker.three.example:443/announce".to_owned(),
        ]
    );

    let many = (0..150)
        .map(|index| format!("udp://t{index}.example:6969/announce"))
        .collect::<Vec<_>>()
        .join("\n");
    let merged = merge_tracker_lists(&[&many]);
    assert_eq!(merged.len(), MAX_TRACKERS);
    assert_eq!(merged[0], "udp://t0.example:6969/announce");

    let long = (0..200)
        .map(|index| {
            format!(
                "udp://tracker-{index:03}.{}.example:6969/announce",
                "a".repeat(40)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let merged = merge_tracker_lists(&[&long]);
    assert!(
        merged.len() < 100,
        "the magnet budget stops the merge first"
    );
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert_eq!(
        drafts.set_trackers(&merged).unwrap(),
        merged,
        "a merged list always saves"
    );
}

#[test]
fn tracker_lists_default_cache_per_list_and_follow_hand_added_trackers() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert_eq!(
        drafts.tracker_lists().unwrap(),
        DEFAULT_TRACKER_LISTS.map(str::to_owned)
    );
    assert!(
        drafts.effective_trackers().unwrap().is_empty(),
        "nothing fetched yet"
    );
    let [first, second] = DEFAULT_TRACKER_LISTS;
    assert_eq!(
        drafts.tracker_list_statuses().unwrap()[0],
        TrackerListStatus {
            url: first.to_owned(),
            tracker_count: 0,
            fetched_at_unix_seconds: None
        }
    );

    let stored = drafts
        .store_tracker_list(
            first,
            "udp://a.example:1/announce\nwss://web.example\nudp://b.example:2/announce",
            100,
        )
        .unwrap();
    assert_eq!(stored, 2, "unsupported lines are dropped");
    drafts
        .store_tracker_list(
            second,
            "udp://b.example:2/announce\nhttps://c.example/announce",
            200,
        )
        .unwrap();
    drafts
        .set_trackers(&["udp://mine.example:3/announce".to_owned()])
        .unwrap();
    assert_eq!(
        drafts.effective_trackers().unwrap(),
        [
            "udp://mine.example:3/announce",
            "udp://a.example:1/announce",
            "udp://b.example:2/announce",
            "https://c.example/announce"
        ]
    );
    assert_eq!(
        drafts.distribution_options().unwrap().tracker_tiers.len(),
        4
    );
    let statuses = drafts.tracker_list_statuses().unwrap();
    assert_eq!(
        (
            statuses[0].tracker_count,
            statuses[0].fetched_at_unix_seconds
        ),
        (2, Some(100))
    );
    assert_eq!(
        (
            statuses[1].tracker_count,
            statuses[1].fetched_at_unix_seconds
        ),
        (2, Some(200))
    );

    // A list that is no longer followed stops contributing; removing every
    // list leaves only the hand-added trackers.
    drafts.set_tracker_lists(&[second.to_owned()]).unwrap();
    assert_eq!(
        drafts.effective_trackers().unwrap(),
        [
            "udp://mine.example:3/announce",
            "udp://b.example:2/announce",
            "https://c.example/announce"
        ]
    );
    drafts.set_tracker_lists(&[]).unwrap();
    assert_eq!(
        drafts.effective_trackers().unwrap(),
        ["udp://mine.example:3/announce"]
    );

    let extra = "https://lists.example/mine.txt".to_owned();
    assert_eq!(
        drafts
            .set_tracker_lists(&[format!(" {extra} "), extra.clone(), String::new()])
            .unwrap(),
        std::slice::from_ref(&extra)
    );
    for bad in [
        vec!["http://lists.example/plain.txt".to_owned()],
        vec!["udp://lists.example:1".to_owned()],
        vec!["lists.example/a.txt".to_owned()],
        (0..11)
            .map(|index| format!("https://lists{index}.example/a.txt"))
            .collect(),
    ] {
        let error = drafts.set_tracker_lists(&bad).unwrap_err();
        assert!(
            matches!(&error, DraftServiceError::InvalidInput { field, .. } if field == "trackerLists"),
            "{bad:?}: {error}"
        );
    }
    assert_eq!(drafts.tracker_lists().unwrap(), [extra]);
}

#[test]
fn the_upload_limit_defaults_to_unlimited_and_persists() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert_eq!(
        drafts.upload_limit().unwrap(),
        publisher_application::RateLimits::default()
    );
    let limits =
        publisher_application::RateLimits::from_kib_per_second(Some(4096), Some(256)).unwrap();
    drafts.set_upload_limit(limits).unwrap();
    assert_eq!(
        drafts.upload_limit().unwrap().as_kib_per_second(),
        (None, Some(256))
    );
    drafts
        .set_upload_limit(publisher_application::RateLimits::default())
        .unwrap();
    assert_eq!(
        drafts.upload_limit().unwrap(),
        publisher_application::RateLimits::default()
    );
}
