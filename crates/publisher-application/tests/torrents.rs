//! Torrent generation for staged attempts.

use std::{fs, path::PathBuf};

use publisher_application::{
    DistributionOptions, DraftServiceError, MetadataEdit, PackagingCancellation, PackagingLimits,
    PublisherDrafts, StagingReceipt, StructureEdit, TorrentError, discard_abandoned_staging,
    generate_torrent, stage_draft, verify_staged_torrent,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-app-torrent-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshot(scratch: &Scratch) -> course_publisher::DraftCourse {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "2.0.0".to_owned(),
                title: "Torrent Course".to_owned(),
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
                title: "Module".to_owned(),
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
                title: "Lesson".to_owned(),
            },
        )
        .unwrap()
        .draft
        .modules[0]
        .lessons[0]
        .id
        .clone();
    let source = scratch.0.join("lecture.pdf");
    fs::write(&source, vec![5_u8; 50_000]).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap()
        .draft
}

fn stage(scratch: &Scratch, draft: &course_publisher::DraftCourse) -> StagingReceipt {
    stage_draft(
        draft,
        &scratch.staging(),
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |_| {},
    )
    .unwrap()
}

fn generate(
    scratch: &Scratch,
    attempt_id: &str,
    distribution: &DistributionOptions,
) -> Result<publisher_application::TorrentReceipt, DraftServiceError> {
    generate_torrent(
        &scratch.staging(),
        attempt_id,
        distribution,
        &PackagingCancellation::default(),
        &mut |_, _| {},
    )
}

#[test]
fn staged_attempts_of_one_snapshot_share_a_verified_info_hash() {
    let scratch = Scratch::new("determinism");
    let draft = snapshot(&scratch);
    let first = stage(&scratch, &draft);
    let second = stage(&scratch, &draft);
    assert_ne!(first.attempt_id, second.attempt_id);

    let receipt = generate(&scratch, &first.attempt_id, &DistributionOptions::default()).unwrap();
    let again = generate(
        &scratch,
        &second.attempt_id,
        &DistributionOptions::default(),
    )
    .unwrap();
    assert_eq!(receipt.info_hash, again.info_hash);
    assert_eq!(
        fs::read(
            scratch
                .staging()
                .join(format!("{}.torrent", first.attempt_id))
        )
        .unwrap(),
        fs::read(
            scratch
                .staging()
                .join(format!("{}.torrent", second.attempt_id))
        )
        .unwrap()
    );
    assert_eq!(receipt.file_count, 2);
    assert!(receipt.total_bytes > 50_000);
    assert_eq!(receipt.piece_length, 16 * 1024);
    assert!(receipt.magnet.contains(&receipt.info_hash));
    assert!(receipt.magnet.contains("&dn="));
    assert_eq!(
        verify_staged_torrent(
            &scratch.staging(),
            &first.attempt_id,
            &PackagingCancellation::default(),
            &mut |_| {}
        )
        .unwrap(),
        receipt.info_hash
    );
    assert!(matches!(
        generate(&scratch, &first.attempt_id, &DistributionOptions::default()),
        Err(DraftServiceError::InvalidAttempt(message)) if message.contains("already")
    ));
}

#[test]
fn trackers_change_the_magnet_but_not_the_info_hash() {
    let scratch = Scratch::new("trackers");
    let draft = snapshot(&scratch);
    let plain = generate(
        &scratch,
        &stage(&scratch, &draft).attempt_id,
        &DistributionOptions::default(),
    )
    .unwrap();
    let tracked = generate(
        &scratch,
        &stage(&scratch, &draft).attempt_id,
        &DistributionOptions {
            tracker_tiers: vec![vec!["udp://tracker.example:6969/announce".to_owned()]],
            web_seeds: vec![],
        },
    )
    .unwrap();
    assert_eq!(plain.info_hash, tracked.info_hash);
    assert!(tracked.magnet.contains("&tr=udp%3A%2F%2Ftracker.example"));
    assert_eq!((plain.tracker_count, tracked.tracker_count), (0, 1));
}

#[test]
fn modified_attempts_are_rejected() {
    let scratch = Scratch::new("tamper");
    let draft = snapshot(&scratch);
    let extra = stage(&scratch, &draft);
    fs::write(
        scratch.staging().join(&extra.attempt_id).join("extra.txt"),
        b"x",
    )
    .unwrap();
    assert!(matches!(
        generate(&scratch, &extra.attempt_id, &DistributionOptions::default()),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
    assert!(
        !scratch
            .staging()
            .join(format!("{}.torrent", extra.attempt_id))
            .exists()
    );

    let changed = stage(&scratch, &draft);
    generate(
        &scratch,
        &changed.attempt_id,
        &DistributionOptions::default(),
    )
    .unwrap();
    let resource = fs::read_dir(
        scratch
            .staging()
            .join(&changed.attempt_id)
            .join("resources"),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
    let file = fs::read_dir(&resource)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = fs::read(&file).unwrap();
    bytes[100] ^= 0xff;
    fs::write(&file, bytes).unwrap();
    assert!(matches!(
        verify_staged_torrent(
            &scratch.staging(),
            &changed.attempt_id,
            &PackagingCancellation::default(),
            &mut |_| {}
        ),
        Err(DraftServiceError::Torrent(TorrentError::Verification(_)))
    ));
}

#[test]
fn attempt_ids_cannot_address_other_paths() {
    let scratch = Scratch::new("ids");
    fs::create_dir_all(scratch.staging()).unwrap();
    for id in [
        "../staging",
        "stage_123",
        "stage_0123456789abcdef0123456789abcdef.partial",
        "",
    ] {
        assert!(
            matches!(
                generate(&scratch, id, &DistributionOptions::default()),
                Err(DraftServiceError::InvalidAttempt(_))
            ),
            "{id}"
        );
    }
    assert!(matches!(
        generate(
            &scratch,
            "stage_0123456789abcdef0123456789abcdef",
            &DistributionOptions::default()
        ),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}

#[test]
fn cancelled_generation_leaves_no_torrent_and_partials_are_discarded() {
    let scratch = Scratch::new("cancel");
    let draft = snapshot(&scratch);
    let attempt = stage(&scratch, &draft);
    let cancellation = PackagingCancellation::default();
    cancellation.cancel();
    assert!(matches!(
        generate_torrent(
            &scratch.staging(),
            &attempt.attempt_id,
            &DistributionOptions::default(),
            &cancellation,
            &mut |_, _| {}
        ),
        Err(DraftServiceError::Torrent(TorrentError::Cancelled))
    ));
    assert!(
        !scratch
            .staging()
            .join(format!("{}.torrent", attempt.attempt_id))
            .exists()
    );

    let leftover = scratch
        .staging()
        .join(format!("{}.torrent.partial", attempt.attempt_id));
    fs::write(&leftover, b"partial").unwrap();
    assert_eq!(discard_abandoned_staging(&scratch.staging()).unwrap(), 1);
    assert!(!leftover.exists());
    assert!(scratch.staging().join(&attempt.attempt_id).is_dir());
}
