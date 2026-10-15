//! Release preparation job orchestration.

use std::{fs, path::PathBuf};

use publisher_application::{
    DistributionOptions, DraftServiceError, MetadataEdit, PublishJobSnapshot, PublishJobs,
    PublishStage, PublisherDrafts, StructureEdit,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-app-jobs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }

    fn staging_entries(&self) -> Vec<String> {
        fs::read_dir(self.staging())
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn publishable(scratch: &Scratch, bytes: usize) -> course_publisher::DraftCourse {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "1.0.0".to_owned(),
                title: "Job Course".to_owned(),
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
    fs::write(&source, vec![3_u8; bytes]).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap()
        .draft
}

#[test]
fn a_job_walks_every_stage_and_reports_versioned_snapshots() {
    let scratch = Scratch::new("happy");
    let jobs = PublishJobs::new(&scratch.staging());
    let draft = publishable(&scratch, 3 * 1024 * 1024);
    let started = jobs.begin(&draft).unwrap();
    assert!(started.job_id.starts_with("job_"));
    assert_eq!(started.stage, PublishStage::Draft);
    assert_eq!(started.draft_revision, 4);

    let mut events: Vec<PublishJobSnapshot> = Vec::new();
    let finished = jobs
        .run(
            &started.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |snapshot| {
                events.push(snapshot.clone());
            },
        )
        .unwrap();

    let mut stages: Vec<PublishStage> = events.iter().map(|event| event.stage).collect();
    stages.dedup();
    assert_eq!(
        stages,
        [
            PublishStage::Validating,
            PublishStage::Packaging,
            PublishStage::GeneratingTorrent,
            PublishStage::Verifying,
            PublishStage::ReadyToPublish,
        ]
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    assert!(events.iter().any(|event| {
        event.stage == PublishStage::Packaging
            && event
                .progress
                .is_some_and(|progress| progress.completed_bytes == progress.total_bytes)
    }));
    assert_eq!(finished, *events.last().unwrap());
    assert_eq!(jobs.snapshot(&started.job_id).unwrap(), finished);
    let prepared = finished.prepared.unwrap();
    assert_eq!(prepared.torrent.attempt_id, prepared.staging.attempt_id);
    assert_eq!(prepared.torrent.info_hash.len(), 40);
    assert!(
        scratch
            .staging()
            .join(&prepared.staging.attempt_id)
            .is_dir()
    );
    assert!(
        scratch
            .staging()
            .join(format!("{}.torrent", prepared.staging.attempt_id))
            .is_file()
    );
    assert!(!jobs.cancel(&started.job_id));
}

#[test]
fn only_one_job_runs_at_a_time() {
    let scratch = Scratch::new("single");
    let jobs = PublishJobs::new(&scratch.staging());
    let draft = publishable(&scratch, 1024);
    let first = jobs.begin(&draft).unwrap();
    assert!(matches!(
        jobs.begin(&draft),
        Err(DraftServiceError::JobActive)
    ));
    jobs.run(
        &first.job_id,
        &draft,
        &DistributionOptions::default(),
        &mut |_| {},
    );
    let second = jobs.begin(&draft).unwrap();
    assert_ne!(first.job_id, second.job_id);
}

#[test]
fn an_invalid_draft_fails_during_validation_without_artifacts() {
    let scratch = Scratch::new("invalid");
    let jobs = PublishJobs::new(&scratch.staging());
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft = drafts.create().unwrap().draft;
    let job = jobs.begin(&draft).unwrap();
    let finished = jobs
        .run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(finished.stage, PublishStage::Failed);
    let failure = finished.failure.unwrap();
    assert_eq!(failure.code, "draft_invalid");
    assert!(failure.message.contains("blocking issue"));
    assert_eq!(scratch.staging_entries().len(), 0);
}

#[test]
fn cancellation_during_any_byte_stage_leaves_no_artifacts() {
    for cancel_at in [
        PublishStage::Packaging,
        PublishStage::GeneratingTorrent,
        PublishStage::Verifying,
    ] {
        let scratch = Scratch::new(&format!("cancel-{cancel_at:?}"));
        let jobs = PublishJobs::new(&scratch.staging());
        let draft = publishable(&scratch, 3 * 1024 * 1024);
        let job = jobs.begin(&draft).unwrap();
        let finished = jobs
            .run(
                &job.job_id,
                &draft,
                &DistributionOptions::default(),
                &mut |snapshot| {
                    if snapshot.stage == cancel_at {
                        jobs.cancel(&snapshot.job_id);
                    }
                },
            )
            .unwrap();
        assert_eq!(finished.stage, PublishStage::Cancelled, "{cancel_at:?}");
        assert!(finished.failure.is_none());
        assert!(finished.prepared.is_none());
        assert!(
            scratch.staging_entries().is_empty(),
            "{cancel_at:?}: {:?}",
            scratch.staging_entries()
        );
        assert!(jobs.begin(&draft).is_ok());
    }
}

#[test]
fn recovery_discards_interrupted_attempts() {
    let scratch = Scratch::new("recover");
    let jobs = PublishJobs::new(&scratch.staging());
    let partial = scratch
        .staging()
        .join("stage_0123456789abcdef0123456789abcdef.partial");
    fs::create_dir_all(&partial).unwrap();
    assert_eq!(jobs.recover().unwrap(), 1);
    assert!(!partial.exists());
    assert_eq!(
        PublishJobs::new(&scratch.0.join("missing"))
            .recover()
            .unwrap(),
        0
    );
}

#[test]
fn a_draft_is_in_progress_until_its_job_is_published_failed_or_cancelled() {
    let scratch = Scratch::new("in-progress");
    let jobs = PublishJobs::new(&scratch.staging());
    let draft = publishable(&scratch, 1024);
    assert!(!jobs.draft_in_progress(&draft.draft_id));
    let job = jobs.begin(&draft).unwrap();
    assert!(jobs.draft_in_progress(&draft.draft_id));
    assert!(!jobs.draft_in_progress("draft_other"));
    let finished = jobs
        .run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(finished.stage, PublishStage::ReadyToPublish);
    assert!(jobs.draft_in_progress(&draft.draft_id));

    let scratch = Scratch::new("in-progress-ended");
    let jobs = PublishJobs::new(&scratch.staging());
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let invalid = drafts.create().unwrap().draft;
    let failed = jobs.begin(&invalid).unwrap();
    jobs.run(
        &failed.job_id,
        &invalid,
        &DistributionOptions::default(),
        &mut |_| {},
    );
    assert!(!jobs.draft_in_progress(&invalid.draft_id));
    let draft = publishable(&scratch, 3 * 1024 * 1024);
    let cancelled = jobs.begin(&draft).unwrap();
    jobs.run(
        &cancelled.job_id,
        &draft,
        &DistributionOptions::default(),
        &mut |snapshot| {
            if snapshot.stage == PublishStage::Packaging {
                jobs.cancel(&snapshot.job_id);
            }
        },
    );
    assert!(!jobs.draft_in_progress(&draft.draft_id));
}

#[test]
fn preparation_refuses_to_fill_the_disk() {
    use publisher_application::{DISK_RESERVE_BYTES, available_bytes, check_space};

    #[cfg(unix)]
    assert!(
        available_bytes(&std::env::temp_dir().join("missing/child")).is_some_and(|bytes| bytes > 0)
    );
    check_space(None, u64::MAX).unwrap();
    check_space(Some(2 * DISK_RESERVE_BYTES), DISK_RESERVE_BYTES / 2).unwrap();
    let refused = check_space(
        Some(DISK_RESERVE_BYTES + DISK_RESERVE_BYTES / 5),
        DISK_RESERVE_BYTES / 2,
    )
    .unwrap_err();
    assert!(matches!(
        refused,
        DraftServiceError::InsufficientSpace { .. }
    ));
    assert_eq!(
        refused.to_string(),
        "preparing this release needs about 0.5 GB of free disk space, but only 1.2 GB is free and 1.0 GB is kept free"
    );
}
