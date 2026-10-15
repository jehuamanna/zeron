//! End-to-end application service tests for local import and recovery.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use learning_application::{
    CancellationFlag, CourseLibrary, ImportLimits, LibraryError, NetworkProfile, PeerReachability,
};
use serde_json::json;
use sha2::{Digest, Sha256};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zeron-library-{}-{suffix}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_course(parent: &Path) -> PathBuf {
    let root = parent.join("course");
    fs::create_dir(&root).unwrap();
    let bytes = b"complete verified media";
    fs::write(root.join("lesson.mp4"), bytes).unwrap();
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").unwrap();
    }
    let manifest = json!({
        "schemaVersion": 1,
        "courseId": "course_recovery",
        "version": "1.0.0",
        "title": "Recovery Course",
        "description": "Durable local learning",
        "publisher": { "name": "Zeron Tests" },
        "language": "en",
        "tags": ["recovery"],
        "modules": [{
            "id": "module_one",
            "title": "Module",
            "position": 0,
            "lessons": [{
                "id": "lesson_one",
                "title": "Lesson",
                "position": 0,
                "resources": [{
                    "id": "resource_video",
                    "kind": "video",
                    "path": "lesson.mp4",
                    "mediaType": "video/mp4",
                    "byteLength": bytes.len(),
                    "sha256": digest
                }]
            }]
        }]
    });
    fs::write(
        root.join("course.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    root
}

#[test]
fn verified_import_survives_database_restart() {
    let temp = TempRoot::new();
    let course_root = create_course(temp.path());
    let database = temp.path().join("library.sqlite3");
    {
        let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
        let receipt = library
            .import_local_folder(&course_root, &CancellationFlag::default())
            .unwrap();
        assert_eq!(receipt.course_id, "course_recovery");
        assert_eq!(receipt.verified_resource_count, 1);
    }
    let library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
    assert_eq!(library.courses().unwrap().len(), 1);
    assert_eq!(library.search("recovery").unwrap().len(), 1);
    assert_eq!(
        library.manifest("course_recovery").unwrap().unwrap().title,
        "Recovery Course"
    );
    assert_eq!(
        library
            .resource_path("course_recovery", "resource_video")
            .unwrap(),
        Some(course_root.join("lesson.mp4"))
    );
    assert_eq!(
        library
            .resource_path("course_recovery", "missing_resource")
            .unwrap(),
        None
    );
    let resource = library
        .local_resource("course_recovery", "resource_video")
        .unwrap()
        .unwrap();
    assert_eq!(resource.path, course_root.join("lesson.mp4"));
    assert_eq!(resource.media_type, "video/mp4");
    assert_eq!(resource.byte_length, 23);
    assert_eq!(library.torrent_info_hash("course_recovery").unwrap(), None);
}

#[test]
fn network_profile_defaults_to_local_network_and_persists() {
    let temp = TempRoot::new();
    let database = temp.path().join("library.sqlite3");
    {
        let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
        assert_eq!(
            library.network_profile().unwrap(),
            NetworkProfile::LocalNetwork
        );
        library
            .set_network_profile(NetworkProfile::Internet)
            .unwrap();
        assert_eq!(
            library.peer_reachability().unwrap(),
            PeerReachability::default()
        );
        library
            .set_peer_reachability(PeerReachability::new(Some(51413), true).unwrap())
            .unwrap();
    }
    let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
    assert_eq!(library.network_profile().unwrap(), NetworkProfile::Internet);
    assert_eq!(
        library.peer_reachability().unwrap(),
        PeerReachability::new(Some(51413), true).unwrap()
    );
    library
        .set_peer_reachability(PeerReachability::default())
        .unwrap();
    assert_eq!(
        library.peer_reachability().unwrap(),
        PeerReachability::default()
    );
}

#[test]
fn failed_import_never_becomes_visible() {
    let temp = TempRoot::new();
    let course_root = create_course(temp.path());
    fs::write(course_root.join("lesson.mp4"), b"tampered").unwrap();
    let mut library = CourseLibrary::open_in_memory(ImportLimits::default()).unwrap();
    assert!(matches!(
        library.import_local_folder(&course_root, &CancellationFlag::default()),
        Err(LibraryError::Import(_))
    ));
    assert!(library.courses().unwrap().is_empty());
}
