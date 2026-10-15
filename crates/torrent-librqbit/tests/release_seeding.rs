//! The Admin seeding service over the real engine, including restart restore.

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, SeedingService, StructureEdit,
    TransferState,
};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};
use torrent_port::NetworkProfile;

async fn engine(root: &Path, name: &str) -> Arc<LibrqbitEngine> {
    Arc::new(
        LibrqbitEngine::start(EngineConfig {
            profile: NetworkProfile::Loopback,
            state_directory: root.join(name),
            listen_port: None,
        })
        .await
        .unwrap(),
    )
}

async fn wait_seeding(service: &SeedingService<LibrqbitEngine>, release_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = service
            .statuses()
            .unwrap()
            .into_iter()
            .find(|status| status.release.release.release_id == release_id)
            .unwrap();
        if status.transfer.as_ref().map(|transfer| transfer.state) == Some(TransferState::Seeding) {
            assert!(status.enabled);
            return;
        }
        assert!(Instant::now() < deadline, "not seeding: {status:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn seeding_survives_an_application_restart() {
    let root = std::env::temp_dir().join(format!("zeron-release-seeding-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("publisher.sqlite3");
    let releases = root.join("releases");

    let release_id = publish(&database, &root, &releases);

    let first_engine = engine(&root, "engine-1").await;
    let drafts = Arc::new(Mutex::new(PublisherDrafts::open(&database).unwrap()));
    let service = SeedingService::new(
        Arc::clone(&drafts),
        Arc::clone(&first_engine),
        releases.clone(),
    );
    service.start(&release_id).await.unwrap();
    wait_seeding(&service, &release_id).await;
    first_engine.shutdown().await;
    drop(service);
    drop(drafts);

    let second_engine = engine(&root, "engine-2").await;
    let drafts = Arc::new(Mutex::new(PublisherDrafts::open(&database).unwrap()));
    let restored = SeedingService::new(
        Arc::clone(&drafts),
        Arc::clone(&second_engine),
        releases.clone(),
    );
    assert_eq!(restored.restore().await.unwrap(), 1);
    wait_seeding(&restored, &release_id).await;
    restored.stop(&release_id).await.unwrap();
    let status = restored.statuses().unwrap().remove(0);
    assert!(!status.enabled && status.transfer.is_none());
    second_engine.shutdown().await;

    make_writable(&root);
    std::fs::remove_dir_all(&root).unwrap();
}

fn publish(database: &Path, root: &Path, releases: &Path) -> String {
    let mut drafts = PublisherDrafts::open(database).unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "1.0.0".to_owned(),
                title: "Restored".to_owned(),
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
    std::fs::write(&source, vec![7_u8; 1_500_000]).unwrap();
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
    jobs.activate(&job.job_id, &mut drafts, releases)
        .unwrap()
        .0
        .release
        .release_id
}

fn make_writable(path: &Path) {
    for entry in std::fs::read_dir(path).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            make_writable(&path);
        } else {
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
            permissions.set_readonly(false);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
    }
}
