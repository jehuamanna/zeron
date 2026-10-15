//! Backend-only source selection tests.

use std::{fs, path::PathBuf};

use course_format::ResourceKind;
use publisher_application::{DraftServiceError, PublisherDrafts, StructureEdit, source_info};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-sources-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn draft_with_lesson(drafts: &mut PublisherDrafts) -> (String, String) {
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let module_id = drafts
        .edit_structure(
            &draft_id,
            0,
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
            1,
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
    (draft_id, lesson_id)
}

#[test]
fn attaches_a_regular_file_as_a_classified_resource() {
    let scratch = Scratch::new("attach");
    let source = scratch.0.join("Lecture 1.MP4");
    fs::write(&source, b"0123456789").unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);

    let saved = drafts
        .attach_source(&draft_id, 2, &lesson_id, &source)
        .unwrap();
    assert_eq!(saved.draft.revision, 3);
    let resource = &saved.draft.modules[0].lessons[0].resources[0];
    assert!(resource.id.starts_with("resource_"));
    assert_eq!(resource.kind, ResourceKind::Video);
    assert_eq!(resource.media_type, "video/mp4");
    assert_eq!(
        resource.package_path,
        format!("resources/{}/Lecture_1.MP4", resource.id)
    );
    assert_eq!(resource.source_path, fs::canonicalize(&source).unwrap());
    let info = source_info(resource);
    assert_eq!(info.file_name, "Lecture 1.MP4");
    assert_eq!(info.byte_length, Some(10));
    assert!(
        drafts
            .validate(&draft_id)
            .unwrap()
            .iter()
            .all(|issue| issue.code != "missing_source")
    );
    assert_eq!(fs::read(&source).unwrap(), b"0123456789");
}

#[test]
fn rejects_directories_and_missing_files_without_revealing_paths() {
    let scratch = Scratch::new("reject");
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);
    for (source, expected) in [
        (scratch.0.clone(), "is not a regular file"),
        (scratch.0.join("missing.mp4"), "is unavailable"),
    ] {
        match drafts.attach_source(&draft_id, 2, &lesson_id, &source) {
            Err(error @ DraftServiceError::SourceRejected(message)) => {
                assert_eq!(message, expected);
                assert!(!error.to_string().contains(&*scratch.0.to_string_lossy()));
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }
    assert_eq!(drafts.draft(&draft_id).unwrap().unwrap().draft.revision, 2);
}

#[test]
fn detached_and_reordered_resources_keep_source_files() {
    let scratch = Scratch::new("detach");
    let first = scratch.0.join("a.pdf");
    let second = scratch.0.join("b.vtt");
    fs::write(&first, b"pdf").unwrap();
    fs::write(&second, b"vtt").unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);
    drafts
        .attach_source(&draft_id, 2, &lesson_id, &first)
        .unwrap();
    let attached = drafts
        .attach_source(&draft_id, 3, &lesson_id, &second)
        .unwrap();
    let ids: Vec<_> = attached.draft.modules[0].lessons[0]
        .resources
        .iter()
        .map(|resource| resource.id.clone())
        .collect();
    let moved = drafts
        .edit_structure(
            &draft_id,
            4,
            StructureEdit::MoveResource {
                lesson_id: lesson_id.clone(),
                resource_id: ids[1].clone(),
                position: 0,
            },
        )
        .unwrap();
    assert_eq!(moved.draft.modules[0].lessons[0].resources[0].id, ids[1]);
    assert_eq!(
        moved.draft.modules[0].lessons[0].resources[0].kind,
        ResourceKind::Subtitle
    );
    let removed = drafts
        .edit_structure(
            &draft_id,
            5,
            StructureEdit::RemoveResource {
                resource_id: ids[0].clone(),
            },
        )
        .unwrap();
    assert_eq!(removed.draft.modules[0].lessons[0].resources.len(), 1);
    assert!(first.is_file());
    assert!(second.is_file());
}

#[test]
fn a_source_removed_after_selection_is_reported_missing() {
    let scratch = Scratch::new("vanish");
    let source = scratch.0.join("talk.mp3");
    fs::write(&source, b"mp3").unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);
    let saved = drafts
        .attach_source(&draft_id, 2, &lesson_id, &source)
        .unwrap();
    fs::remove_file(&source).unwrap();
    let info = source_info(&saved.draft.modules[0].lessons[0].resources[0]);
    assert_eq!(info.file_name, "talk.mp3");
    assert_eq!(info.byte_length, None);
    assert!(
        drafts
            .validate(&draft_id)
            .unwrap()
            .iter()
            .any(|issue| issue.code == "missing_source")
    );
}

#[cfg(unix)]
#[test]
fn symlinked_selection_records_the_resolved_file() {
    let scratch = Scratch::new("symlink");
    let target = scratch.0.join("real.mp4");
    let link = scratch.0.join("link.mp4");
    fs::write(&target, b"mp4").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);
    let saved = drafts
        .attach_source(&draft_id, 2, &lesson_id, &link)
        .unwrap();
    let resource = &saved.draft.modules[0].lessons[0].resources[0];
    assert_eq!(resource.source_path, fs::canonicalize(&target).unwrap());
    assert_eq!(source_info(resource).file_name, "real.mp4");
}
