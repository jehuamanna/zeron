//! Local course import integration tests.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use learning_application::{CancellationFlag, ImportError, ImportLimits, import_local_course};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempCourse(PathBuf);

impl TempCourse {
    fn new() -> Self {
        let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zeron-course-import-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempCourse {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_course(resource_path: &str, bytes: &[u8]) -> TempCourse {
    let course = TempCourse::new();
    let full_resource_path = course.path().join(resource_path);
    fs::create_dir_all(full_resource_path.parent().unwrap()).unwrap();
    fs::write(&full_resource_path, bytes).unwrap();
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").unwrap();
    }
    let manifest = json!({
        "schemaVersion": 1,
        "courseId": "course_test",
        "version": "1.0.0",
        "title": "Test Course",
        "publisher": { "name": "Test Publisher" },
        "language": "en",
        "modules": [{
            "id": "module_one",
            "title": "Module",
            "position": 0,
            "lessons": [{
                "id": "lesson_one",
                "title": "Lesson",
                "position": 0,
                "resources": [{
                    "id": "resource_one",
                    "kind": "video",
                    "path": resource_path,
                    "mediaType": "video/mp4",
                    "byteLength": bytes.len(),
                    "sha256": digest
                }]
            }]
        }]
    });
    fs::write(
        course.path().join("course.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    course
}

#[test]
fn verifies_a_complete_local_course() {
    let course = write_course("modules/lesson.mp4", b"verified media bytes");
    let imported = import_local_course(
        course.path(),
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();
    assert_eq!(imported.manifest.course_id, "course_test");
    assert_eq!(imported.resources.len(), 1);
    assert_eq!(imported.total_resource_bytes, 20);
    assert_eq!(imported.resources[0].package_path, "modules/lesson.mp4");
}

#[test]
fn rejects_length_and_digest_mismatches() {
    let course = write_course("lesson.mp4", b"original");
    let manifest_path = course.path().join("course.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["modules"][0]["lessons"][0]["resources"][0]["byteLength"] = json!(9);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        import_local_course(
            course.path(),
            ImportLimits::default(),
            &CancellationFlag::default()
        ),
        Err(ImportError::ResourceMismatch {
            field: "byte length",
            ..
        })
    ));

    manifest["modules"][0]["lessons"][0]["resources"][0]["byteLength"] = json!(8);
    manifest["modules"][0]["lessons"][0]["resources"][0]["sha256"] =
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(matches!(
        import_local_course(
            course.path(),
            ImportLimits::default(),
            &CancellationFlag::default()
        ),
        Err(ImportError::ResourceMismatch {
            field: "SHA-256",
            ..
        })
    ));
}

#[test]
fn enforces_manifest_and_package_limits() {
    let course = write_course("lesson.mp4", b"original");
    assert!(matches!(
        import_local_course(
            course.path(),
            ImportLimits {
                max_manifest_bytes: 8,
                ..ImportLimits::default()
            },
            &CancellationFlag::default(),
        ),
        Err(ImportError::ManifestTooLarge { .. })
    ));
    assert!(matches!(
        import_local_course(
            course.path(),
            ImportLimits {
                max_total_resource_bytes: 7,
                ..ImportLimits::default()
            },
            &CancellationFlag::default(),
        ),
        Err(ImportError::LimitExceeded(_))
    ));
}

#[test]
fn honors_preexisting_cancellation() {
    let course = write_course("lesson.mp4", b"original");
    let cancellation = CancellationFlag::default();
    cancellation.cancel();
    assert!(matches!(
        import_local_course(course.path(), ImportLimits::default(), &cancellation),
        Err(ImportError::Cancelled)
    ));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_resource_components() {
    use std::os::unix::fs::symlink;

    let course = write_course("safe/lesson.mp4", b"original");
    fs::rename(course.path().join("safe"), course.path().join("real")).unwrap();
    symlink(course.path().join("real"), course.path().join("safe")).unwrap();
    assert!(matches!(
        import_local_course(
            course.path(),
            ImportLimits::default(),
            &CancellationFlag::default()
        ),
        Err(ImportError::UnsafeResource { .. })
    ));
}
