//! Draft editing and publishing workflow policy tests.

use std::{fs, path::PathBuf};

use course_format::ResourceKind;
use course_publisher::{
    DraftCourse, DraftLesson, DraftMetadata, DraftModule, DraftResource, EditError, PublishStage,
    PublishingWorkflow,
};

fn draft(source_path: PathBuf) -> DraftCourse {
    DraftCourse {
        draft_id: "draft_one".to_owned(),
        revision: 4,
        metadata: DraftMetadata {
            course_id: "course_one".to_owned(),
            version: "1.0.0".to_owned(),
            title: "Course".to_owned(),
            description: None,
            publisher_name: "Publisher".to_owned(),
            language: "en".to_owned(),
            category: None,
            tags: vec![],
            prerequisites: vec![],
            license_name: None,
        },
        modules: vec![
            DraftModule {
                id: "module_a".to_owned(),
                title: "A".to_owned(),
                lessons: vec![],
            },
            DraftModule {
                id: "module_b".to_owned(),
                title: "B".to_owned(),
                lessons: vec![DraftLesson {
                    id: "lesson_b".to_owned(),
                    title: "Lesson".to_owned(),
                    estimated_duration_seconds: None,
                    resources: vec![DraftResource {
                        id: "video_b".to_owned(),
                        kind: ResourceKind::Video,
                        source_path,
                        package_path: "media/video.mp4".to_owned(),
                        media_type: "video/mp4".to_owned(),
                        duration_seconds: None,
                        language: None,
                    }],
                }],
            },
        ],
    }
}

#[test]
fn reorder_preserves_identity_and_rejects_stale_revision() {
    let mut course = draft(PathBuf::from("missing"));
    assert_eq!(course.move_module(4, "module_b", 0), Ok(5));
    assert_eq!(course.modules[0].id, "module_b");
    assert_eq!(
        course.move_module(4, "module_a", 0),
        Err(EditError::RevisionConflict {
            expected: 4,
            actual: 5
        })
    );
}

#[test]
fn validation_reports_missing_sources_and_empty_modules() {
    let course = draft(PathBuf::from("missing"));
    let codes: Vec<_> = course
        .validate()
        .into_iter()
        .map(|issue| issue.code)
        .collect();
    assert!(codes.contains(&"missing_source"));
    assert!(codes.contains(&"empty_module"));
}

#[test]
fn valid_sources_remove_file_issue() {
    let path = std::env::temp_dir().join(format!("zeron-draft-source-{}", std::process::id()));
    fs::write(&path, b"media").unwrap();
    let mut course = draft(path.clone());
    course.modules.remove(0);
    assert_eq!(course.validate().len(), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn workflow_requires_every_verified_stage() {
    let mut workflow = PublishingWorkflow::default();
    assert!(workflow.transition(PublishStage::Packaging).is_err());
    for stage in [
        PublishStage::Validating,
        PublishStage::Packaging,
        PublishStage::GeneratingTorrent,
        PublishStage::Verifying,
        PublishStage::ReadyToPublish,
        PublishStage::Published,
        PublishStage::Seeding,
    ] {
        workflow.transition(stage).unwrap();
    }
    assert_eq!(workflow.stage(), PublishStage::Seeding);
    assert!(workflow.transition(PublishStage::Draft).is_err());
}

#[test]
fn failed_mutable_attempt_can_retry_from_draft() {
    let mut workflow = PublishingWorkflow::restore(PublishStage::Packaging);
    workflow.transition(PublishStage::Failed).unwrap();
    workflow.transition(PublishStage::Draft).unwrap();
    assert_eq!(workflow.stage(), PublishStage::Draft);
}
