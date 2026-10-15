//! Importing the old Learner and Publisher data into the one app's directory.

use std::{
    fs,
    path::{Path, PathBuf},
};

use data_import::{ImportError, ImportOutcome, ImportPlan, JOURNAL_FILE, import_old_data};
use learning_storage::Storage;
use publisher_storage::PublisherStorage;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("zeron-data-import-{name}-{}", std::process::id()));
        let _ = make_writable(&root);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn plan(&self) -> ImportPlan {
        ImportPlan::beside(&self.0.join("app.zeron"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = make_writable(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_writable(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mut permissions = metadata.permissions();
        #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        for entry in fs::read_dir(path)? {
            make_writable(&entry?.path())?;
        }
    }
    Ok(())
}

fn manifest() -> course_format::CourseManifest {
    course_format::parse_and_validate(include_bytes!(
        "../../course-format/tests/fixtures/valid-course.json"
    ))
    .unwrap()
}

/// An old Learner directory with a torrent course downloaded below it, a
/// local course elsewhere, and engine state that is not moved.
fn old_learner(plan: &ImportPlan, settings: &[(&str, &str)]) -> String {
    fs::create_dir_all(plan.learner.join("courses/abc")).unwrap();
    fs::write(plan.learner.join("courses/abc/lesson.mp4"), b"video").unwrap();
    fs::create_dir_all(plan.learner.join("engine")).unwrap();
    let mut library = Storage::open(&plan.learner.join("library.sqlite3")).unwrap();
    let course = manifest();
    library
        .save_torrent_course(
            &course,
            5,
            &plan.learner.join("courses/abc"),
            &"a".repeat(40),
            b"metainfo",
        )
        .unwrap();
    for (key, value) in settings {
        library.set_setting(key, value).unwrap();
    }
    // Left open in WAL mode, like a running app's last write.
    std::mem::forget(library);
    course.course_id
}

/// An old Publisher directory with a read-only release and staging.
fn old_publisher(plan: &ImportPlan, settings: &[(&str, &str)]) {
    let release = plan.publisher.join("releases/release_one/package");
    fs::create_dir_all(&release).unwrap();
    fs::write(release.join("course.json"), b"{}").unwrap();
    for directory in [release.as_path(), release.parent().unwrap()] {
        let mut permissions = fs::metadata(directory).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(directory, permissions).unwrap();
    }
    fs::create_dir_all(plan.publisher.join("staging/job")).unwrap();
    fs::create_dir_all(plan.publisher.join("engine")).unwrap();
    let mut storage = PublisherStorage::open(&plan.publisher.join("publisher.sqlite3")).unwrap();
    storage.set_setting("trackers", "[]").unwrap();
    for (key, value) in settings {
        storage.set_setting(key, value).unwrap();
    }
}

fn library_setting(plan: &ImportPlan, key: &str) -> Option<String> {
    Storage::open(&plan.target.join("library.sqlite3"))
        .unwrap()
        .setting(key)
        .unwrap()
}

#[test]
fn a_fresh_install_records_that_there_was_nothing_to_import() {
    let scratch = Scratch::new("fresh");
    let plan = scratch.plan();
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::NothingToImport
    );
    assert!(plan.target.join(JOURNAL_FILE).is_file());
    assert!(!plan.target.join("library.sqlite3").exists());

    // Old data appearing later is not imported over the new app's own.
    old_publisher(&plan, &[]);
    assert_eq!(import_old_data(&plan).unwrap(), ImportOutcome::AlreadyDone);
    assert!(plan.publisher.join("publisher.sqlite3").exists());
}

#[test]
fn both_old_directories_move_into_the_new_one_once() {
    let scratch = Scratch::new("both");
    let plan = scratch.plan();
    let course_id = old_learner(&plan, &[("download_limit_kib", "512")]);
    old_publisher(
        &plan,
        &[
            ("network_profile", "internet"),
            ("peer_port", "51413"),
            ("port_forwarding", "1"),
            ("upload_limit_kib", "256"),
        ],
    );

    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::Imported {
            learner: true,
            publisher: true
        }
    );
    // Course files moved, and the library points at their new place.
    assert_eq!(
        fs::read(plan.target.join("courses/abc/lesson.mp4")).unwrap(),
        b"video"
    );
    let library = Storage::open(&plan.target.join("library.sqlite3")).unwrap();
    assert_eq!(
        library.course_package_root(&course_id).unwrap(),
        Some(plan.target.join("courses/abc"))
    );
    assert_eq!(library.list_courses().unwrap().len(), 1);
    drop(library);
    // Library had no network choice, so the Publisher's applies; its own
    // speed limit stays, and the Publisher's upload cap fills the gap.
    assert_eq!(
        library_setting(&plan, "network_profile").as_deref(),
        Some("internet")
    );
    assert_eq!(
        library_setting(&plan, "peer_port").as_deref(),
        Some("51413")
    );
    assert_eq!(
        library_setting(&plan, "download_limit_kib").as_deref(),
        Some("512")
    );
    assert_eq!(
        library_setting(&plan, "upload_limit_kib").as_deref(),
        Some("256")
    );

    // Publisher data and its read-only release moved, permissions intact.
    let release = plan.target.join("releases/release_one/package");
    assert_eq!(fs::read(release.join("course.json")).unwrap(), b"{}");
    assert!(fs::metadata(&release).unwrap().permissions().readonly());
    assert!(plan.target.join("staging/job").is_dir());
    let publisher = PublisherStorage::open(&plan.target.join("publisher.sqlite3")).unwrap();
    assert_eq!(
        publisher.setting("trackers").unwrap().as_deref(),
        Some("[]")
    );
    drop(publisher);

    // Old databases are gone; engine state was neither moved nor removed.
    for old in [
        plan.learner.join("library.sqlite3"),
        plan.learner.join("library.sqlite3-wal"),
        plan.publisher.join("publisher.sqlite3"),
    ] {
        assert!(!old.exists(), "{} remains", old.display());
    }
    assert!(plan.learner.join("engine").is_dir());
    assert!(!plan.learner.join("courses").exists());
    assert!(!plan.target.join(".data-import").exists());

    // A second run changes nothing.
    assert_eq!(import_old_data(&plan).unwrap(), ImportOutcome::AlreadyDone);
    assert_eq!(
        fs::read(plan.target.join("courses/abc/lesson.mp4")).unwrap(),
        b"video"
    );
}

#[test]
fn the_learner_keeps_its_own_network_choice() {
    let scratch = Scratch::new("learner-network");
    let plan = scratch.plan();
    old_learner(&plan, &[("peer_port", "6881")]);
    old_publisher(
        &plan,
        &[("network_profile", "internet"), ("peer_port", "51413")],
    );
    import_old_data(&plan).unwrap();
    assert_eq!(library_setting(&plan, "peer_port").as_deref(), Some("6881"));
    assert_eq!(library_setting(&plan, "network_profile"), None);
}

#[test]
fn only_the_learner_directory_is_imported() {
    let scratch = Scratch::new("learner-only");
    let plan = scratch.plan();
    let course_id = old_learner(&plan, &[]);
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::Imported {
            learner: true,
            publisher: false
        }
    );
    let library = Storage::open(&plan.target.join("library.sqlite3")).unwrap();
    assert_eq!(
        library.course_package_root(&course_id).unwrap(),
        Some(plan.target.join("courses/abc"))
    );
    assert!(!plan.target.join("publisher.sqlite3").exists());
}

#[test]
fn only_the_publisher_directory_is_imported_with_its_network_settings() {
    let scratch = Scratch::new("publisher-only");
    let plan = scratch.plan();
    old_publisher(
        &plan,
        &[
            ("network_profile", "local_network"),
            ("port_forwarding", "0"),
        ],
    );
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::Imported {
            learner: false,
            publisher: true
        }
    );
    assert!(plan.target.join("publisher.sqlite3").is_file());
    assert!(
        plan.target
            .join("releases/release_one/package/course.json")
            .is_file()
    );
    // A new library carries the Publisher's network choice.
    assert_eq!(
        library_setting(&plan, "network_profile").as_deref(),
        Some("local_network")
    );
    assert!(!plan.learner.exists());
}

#[test]
fn a_crash_while_copying_starts_the_copy_again() {
    let scratch = Scratch::new("crash-copying");
    let plan = scratch.plan();
    let course_id = old_learner(&plan, &[]);
    // A half-written copy from an interrupted run, with no journal yet.
    fs::create_dir_all(plan.target.join(".data-import")).unwrap();
    fs::write(plan.target.join(".data-import/library.sqlite3"), b"partial").unwrap();
    import_old_data(&plan).unwrap();
    let library = Storage::open(&plan.target.join("library.sqlite3")).unwrap();
    assert!(library.course_package_root(&course_id).unwrap().is_some());
}

#[test]
fn a_crash_part_way_through_the_switch_resumes_where_it_stopped() {
    let scratch = Scratch::new("crash-switching");
    let plan = scratch.plan();
    let course_id = old_learner(&plan, &[]);
    old_publisher(&plan, &[]);
    // Something already in the way stops the switch after courses moved.
    fs::create_dir_all(plan.target.join("releases/other")).unwrap();
    let stopped = import_old_data(&plan).unwrap_err();
    assert!(
        matches!(stopped, ImportError::Conflict("releases")),
        "{stopped}"
    );
    assert!(plan.target.join("courses/abc").is_dir());
    assert!(
        !plan.target.join("library.sqlite3").exists(),
        "databases switch in last"
    );
    assert!(
        plan.learner.join("library.sqlite3").exists(),
        "old data is kept until the switch"
    );

    fs::remove_dir_all(plan.target.join("releases")).unwrap();
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::Imported {
            learner: true,
            publisher: true
        }
    );
    let library = Storage::open(&plan.target.join("library.sqlite3")).unwrap();
    assert_eq!(
        library.course_package_root(&course_id).unwrap(),
        Some(plan.target.join("courses/abc"))
    );
    assert!(
        plan.target
            .join("releases/release_one/package/course.json")
            .is_file()
    );
    assert!(!plan.learner.join("library.sqlite3").exists());
    assert_eq!(import_old_data(&plan).unwrap(), ImportOutcome::AlreadyDone);
}

#[test]
fn a_crash_while_cleaning_up_finishes_the_cleanup() {
    let scratch = Scratch::new("crash-cleaning");
    let plan = scratch.plan();
    old_learner(&plan, &[]);
    import_old_data(&plan).unwrap();
    // As if the run stopped after the switch, before old files were removed.
    fs::write(plan.learner.join("library.sqlite3"), b"old").unwrap();
    fs::write(
        plan.target.join(JOURNAL_FILE),
        br#"{"version":1,"stage":"cleaning","learner":true,"publisher":false}"#,
    )
    .unwrap();
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::Imported {
            learner: true,
            publisher: false
        }
    );
    assert!(!plan.learner.join("library.sqlite3").exists());
    assert!(plan.target.join("library.sqlite3").is_file());
}

#[test]
fn existing_data_in_the_new_directory_is_never_replaced() {
    let scratch = Scratch::new("existing");
    let plan = scratch.plan();
    old_learner(&plan, &[]);
    fs::create_dir_all(&plan.target).unwrap();
    Storage::open(&plan.target.join("library.sqlite3"))
        .unwrap()
        .set_setting("peer_port", "7000")
        .unwrap();
    assert_eq!(
        import_old_data(&plan).unwrap(),
        ImportOutcome::NothingToImport
    );
    assert_eq!(library_setting(&plan, "peer_port").as_deref(), Some("7000"));
    assert!(plan.learner.join("library.sqlite3").exists());
}
