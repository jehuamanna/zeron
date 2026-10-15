//! Deterministic package staging tests with independent importer verification.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use course_format::{ResourceKind, parse_and_validate};
use course_publisher::{
    DraftCourse, DraftLesson, DraftMetadata, DraftModule, DraftResource, MANIFEST_FILE_NAME,
    PackagingCancellation, PackagingError, PackagingLimits, StagedPackage, stage_package,
};
use learning_application::{CancellationFlag, ImportLimits, import_local_course};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-packaging-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("sources")).unwrap();
        Self(path)
    }

    fn source(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("sources").join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn out(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn resource(id: &str, kind: ResourceKind, source: PathBuf, media_type: &str) -> DraftResource {
    let file_name = source.file_name().unwrap().to_string_lossy().into_owned();
    DraftResource {
        id: id.to_owned(),
        kind,
        source_path: source,
        package_path: format!("resources/{id}/{file_name}"),
        media_type: media_type.to_owned(),
        duration_seconds: Some(90),
        language: None,
    }
}

fn draft(scratch: &Scratch) -> DraftCourse {
    DraftCourse {
        draft_id: "draft_one".to_owned(),
        revision: 7,
        metadata: DraftMetadata {
            course_id: "course_rust".to_owned(),
            version: "1.2.0".to_owned(),
            title: "Rust Systems".to_owned(),
            description: Some("Ownership in depth.".to_owned()),
            publisher_name: "Zeron Tests".to_owned(),
            language: "en".to_owned(),
            category: Some("Programming".to_owned()),
            tags: vec!["rust".to_owned()],
            prerequisites: vec![],
            license_name: Some("CC-BY-4.0".to_owned()),
        },
        modules: vec![
            DraftModule {
                id: "module_basics".to_owned(),
                title: "Basics".to_owned(),
                lessons: vec![DraftLesson {
                    id: "lesson_intro".to_owned(),
                    title: "Intro".to_owned(),
                    estimated_duration_seconds: Some(600),
                    resources: vec![
                        resource(
                            "resource_video",
                            ResourceKind::Video,
                            scratch.source("intro.mp4", &[7_u8; 3000]),
                            "video/mp4",
                        ),
                        resource(
                            "resource_subs",
                            ResourceKind::Subtitle,
                            scratch.source("intro.vtt", b"WEBVTT\n"),
                            "text/vtt",
                        ),
                    ],
                }],
            },
            DraftModule {
                id: "module_more".to_owned(),
                title: "More".to_owned(),
                lessons: vec![DraftLesson {
                    id: "lesson_notes".to_owned(),
                    title: "Notes".to_owned(),
                    estimated_duration_seconds: None,
                    resources: vec![resource(
                        "resource_pdf",
                        ResourceKind::Pdf,
                        scratch.source("notes.pdf", b"%PDF-1.7"),
                        "application/pdf",
                    )],
                }],
            },
        ],
    }
}

fn stage(course: &DraftCourse, destination: &Path) -> Result<StagedPackage, PackagingError> {
    stage_package(
        course,
        destination,
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |_| {},
    )
}

#[test]
fn staged_package_passes_independent_import_verification() {
    let scratch = Scratch::new("verify");
    let course = draft(&scratch);
    let destination = scratch.out("staged");
    let mut events = Vec::new();
    let staged = stage_package(
        &course,
        &destination,
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |progress| events.push(progress),
    )
    .unwrap();

    assert_eq!(staged.files.len(), 3);
    assert_eq!(staged.total_resource_bytes, 3000 + 7 + 8);
    let last = events.last().unwrap();
    assert_eq!(last.completed_files, 3);
    assert_eq!(last.completed_bytes, last.total_bytes);

    let manifest_bytes = fs::read(destination.join(MANIFEST_FILE_NAME)).unwrap();
    let parsed = parse_and_validate(&manifest_bytes).unwrap();
    assert_eq!(parsed, staged.manifest);
    assert_eq!(parsed.modules[1].position, 1);
    assert_eq!(
        parsed.modules[0].lessons[0].resources[1].path,
        "resources/resource_subs/intro.vtt"
    );
    assert_eq!(parsed.license.as_ref().unwrap().name, "CC-BY-4.0");

    let imported = import_local_course(
        &destination,
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();
    assert_eq!(imported.manifest, staged.manifest);
    for (verified, file) in imported.resources.iter().zip(&staged.files) {
        assert_eq!(verified.resource_id, file.resource_id);
        assert_eq!(verified.sha256, file.sha256);
        assert_eq!(verified.byte_length, file.byte_length);
    }
    assert_eq!(
        fs::read(scratch.0.join("sources/intro.mp4")).unwrap(),
        [7_u8; 3000]
    );
}

#[test]
fn identical_drafts_stage_byte_identical_manifests_and_trees() {
    let scratch = Scratch::new("determinism");
    let course = draft(&scratch);
    let first = stage(&course, &scratch.out("first")).unwrap();
    let second = stage(&course, &scratch.out("second")).unwrap();
    assert_eq!(first.manifest_sha256, second.manifest_sha256);
    assert_eq!(
        fs::read(scratch.out("first").join(MANIFEST_FILE_NAME)).unwrap(),
        fs::read(scratch.out("second").join(MANIFEST_FILE_NAME)).unwrap()
    );
    assert_eq!(first.files, second.files);
    for file in &first.files {
        assert_eq!(
            fs::read(scratch.out("first").join(&file.package_path)).unwrap(),
            fs::read(scratch.out("second").join(&file.package_path)).unwrap()
        );
    }
}

#[test]
fn invalid_drafts_and_metadata_fail_before_anything_is_written() {
    let scratch = Scratch::new("invalid");
    let mut course = draft(&scratch);
    course.modules[1].lessons.clear();
    assert!(matches!(
        stage(&course, &scratch.out("empty_module")),
        Err(PackagingError::DraftInvalid(issues)) if issues.iter().any(|issue| issue.code == "empty_module")
    ));
    assert!(!scratch.out("empty_module").exists());

    let mut course = draft(&scratch);
    course.metadata.version = "one".to_owned();
    course.modules[0].lessons[0].resources[0].package_path = "../escape.mp4".to_owned();
    match stage(&course, &scratch.out("bad_manifest")) {
        Err(PackagingError::Manifest(issues)) => {
            assert!(issues.iter().any(|issue| issue.path == "version"));
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.path.rsplit('.').next() == Some("path"))
            );
        }
        other => panic!("expected manifest issues, got {other:?}"),
    }
    assert!(!scratch.out("bad_manifest").exists());
    assert!(!scratch.0.join("escape.mp4").exists());
}

#[test]
fn cancellation_removes_the_partial_package() {
    let scratch = Scratch::new("cancel");
    let course = draft(&scratch);
    let destination = scratch.out("staged");
    let cancellation = PackagingCancellation::default();
    let result = stage_package(
        &course,
        &destination,
        PackagingLimits::default(),
        &cancellation,
        &mut |progress| {
            if progress.completed_files == 1 {
                cancellation.cancel();
            }
        },
    );
    assert!(matches!(result, Err(PackagingError::Cancelled)));
    assert!(!destination.exists());
    assert!(scratch.0.join("sources/intro.mp4").is_file());
}

#[test]
fn a_source_growing_during_copy_is_rejected() {
    let scratch = Scratch::new("changed");
    let course = draft(&scratch);
    let destination = scratch.out("staged");
    let video = scratch.0.join("sources/intro.mp4");
    let mut appended = false;
    let result = stage_package(
        &course,
        &destination,
        PackagingLimits::default(),
        &PackagingCancellation::default(),
        &mut |progress| {
            if progress.completed_bytes > 0 && !appended {
                appended = true;
                let mut file = OpenOptions::new().append(true).open(&video).unwrap();
                file.write_all(b"late bytes").unwrap();
            }
        },
    );
    assert!(matches!(
        result,
        Err(PackagingError::SourceChanged { resource_id }) if resource_id == "resource_video"
    ));
    assert!(!destination.exists());
}

#[test]
fn existing_destinations_are_never_reused_or_removed() {
    let scratch = Scratch::new("exists");
    let course = draft(&scratch);
    let destination = scratch.out("staged");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep.txt"), b"keep").unwrap();
    assert!(matches!(
        stage(&course, &destination),
        Err(PackagingError::DestinationExists)
    ));
    assert_eq!(fs::read(destination.join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn limits_are_enforced_before_copying() {
    let scratch = Scratch::new("limits");
    let course = draft(&scratch);
    for limits in [
        PackagingLimits {
            max_resource_count: 2,
            ..PackagingLimits::default()
        },
        PackagingLimits {
            max_total_bytes: 3000,
            ..PackagingLimits::default()
        },
    ] {
        let destination = scratch.out("limited");
        let result = stage_package(
            &course,
            &destination,
            limits,
            &PackagingCancellation::default(),
            &mut |_| {},
        );
        assert!(matches!(result, Err(PackagingError::LimitExceeded(_))));
        assert!(!destination.exists());
    }
}
