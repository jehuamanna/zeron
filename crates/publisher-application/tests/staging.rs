//! Staging attempts built from service-created drafts.

use std::{fs, path::PathBuf};

use learning_application::{CancellationFlag, ImportLimits, import_local_course};
use publisher_application::{
    DraftServiceError, MetadataEdit, PackagingCancellation, PackagingError, PackagingLimits,
    PublisherDrafts, StructureEdit, discard_abandoned_staging, stage_draft,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-app-staging-{name}-{}", std::process::id()));
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

fn publishable_draft(drafts: &mut PublisherDrafts, scratch: &Scratch) -> String {
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "1.0.0".to_owned(),
                title: "Staged Course".to_owned(),
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
    let source = scratch.0.join("Lecture 01: Welcome!.mp4");
    fs::write(&source, vec![1_u8; 4096]).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap();
    draft_id
}

#[test]
fn a_service_built_draft_stages_into_an_importable_attempt() {
    let scratch = Scratch::new("import");
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = publishable_draft(&mut drafts, &scratch);
    let snapshot = drafts.draft(&draft_id).unwrap().unwrap().draft;
    let staging_root = scratch.0.join("staging");

    let receipt = stage_draft(
        &snapshot,
        &staging_root,
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(receipt.attempt_id.starts_with("stage_"));
    assert_eq!(receipt.draft_revision, 4);
    assert_eq!(receipt.course_version, "1.0.0");
    assert_eq!(receipt.resource_count, 1);
    assert_eq!(receipt.total_resource_bytes, 4096);
    assert_eq!(receipt.manifest_sha256.len(), 64);

    let attempt = staging_root.join(&receipt.attempt_id);
    assert!(attempt.is_dir());
    assert!(
        !staging_root
            .join(format!("{}.partial", receipt.attempt_id))
            .exists()
    );
    let imported = import_local_course(
        &attempt,
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();
    assert_eq!(imported.manifest.title, "Staged Course");
    assert_eq!(imported.total_resource_bytes, 4096);
    assert_eq!(
        imported.resources[0].package_path.rsplit('/').next(),
        Some("Lecture_01__Welcome_.mp4")
    );
}

#[test]
fn an_unpublishable_draft_leaves_no_attempt_behind() {
    let scratch = Scratch::new("invalid");
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let snapshot = drafts.draft(&draft_id).unwrap().unwrap().draft;
    let staging_root = scratch.0.join("staging");
    let result = stage_draft(
        &snapshot,
        &staging_root,
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |_| {},
    );
    assert!(matches!(
        result,
        Err(DraftServiceError::Packaging(PackagingError::DraftInvalid(
            _
        )))
    ));
    assert_eq!(fs::read_dir(&staging_root).unwrap().count(), 0);
}

#[test]
fn abandoned_partial_attempts_are_discarded_and_everything_else_is_kept() {
    let scratch = Scratch::new("abandoned");
    let root = scratch.0.join("staging");
    let hex = "0123456789abcdef0123456789abcdef";
    let abandoned = root.join(format!("stage_{hex}.partial"));
    let completed = root.join(format!("stage_{hex}"));
    let unrelated = root.join("notes.partial");
    fs::create_dir_all(abandoned.join("resources")).unwrap();
    fs::write(abandoned.join("resources/file.bin"), b"partial").unwrap();
    fs::create_dir_all(&completed).unwrap();
    fs::create_dir_all(&unrelated).unwrap();
    let outside = scratch.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        &outside,
        root.join("stage_fedcba9876543210fedcba9876543210.partial"),
    )
    .unwrap();

    assert_eq!(discard_abandoned_staging(&root).unwrap(), 1);
    assert!(!abandoned.exists());
    assert!(completed.is_dir());
    assert!(unrelated.is_dir());
    assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"keep");
    assert_eq!(
        discard_abandoned_staging(&scratch.0.join("missing")).unwrap(),
        0
    );
}
