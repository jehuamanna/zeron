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
            PeerReachability {
                port: None,
                port_forwarding: true
            },
            "an ephemeral port with forwarding on by default"
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
    assert_eq!(library.courses().unwrap().len(), 0);
}

#[test]
fn playback_positions_persist_across_restarts_and_reset_when_finished() {
    let root = TempRoot::new();
    let course_root = create_course(root.path());
    let database = root.path().join("library.sqlite3");
    {
        let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
        library
            .import_local_folder(&course_root, &CancellationFlag::default())
            .unwrap();
        assert_eq!(
            library
                .playback_position("course_recovery", "resource_video")
                .unwrap(),
            None
        );
        assert!(
            library
                .save_playback_position("course_recovery", "resource_video", 125.4, 600.0)
                .unwrap()
        );
        assert!(
            !library
                .save_playback_position("course_recovery", "missing_resource", 1.0, 600.0)
                .unwrap()
        );
        assert!(
            !library
                .save_playback_position("course_missing", "resource_video", 1.0, 600.0)
                .unwrap()
        );
        for (position, duration) in [
            (f64::NAN, 600.0),
            (-1.0, 600.0),
            (f64::INFINITY, 600.0),
            (700.0, 600.0),
            (1.0, 0.0),
            (1.0, f64::INFINITY),
            (1.0, 49.0 * 3600.0),
        ] {
            assert!(matches!(
                library.save_playback_position(
                    "course_recovery",
                    "resource_video",
                    position,
                    duration
                ),
                Err(LibraryError::InvalidPlaybackTime(_))
            ));
        }
    }
    let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
    assert_eq!(
        library
            .playback_position("course_recovery", "resource_video")
            .unwrap(),
        Some(125.4)
    );
    library
        .save_playback_position("course_recovery", "resource_video", 595.0, 600.0)
        .unwrap();
    assert_eq!(
        library
            .playback_position("course_recovery", "resource_video")
            .unwrap(),
        Some(0.0)
    );
}

#[test]
fn rate_limits_default_to_unlimited_and_persist() {
    use learning_application::RateLimits;

    let root = TempRoot::new();
    let database = root.path().join("library.sqlite3");
    let limits = RateLimits::from_kib_per_second(Some(2048), Some(128)).unwrap();
    {
        let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
        assert_eq!(library.rate_limits().unwrap(), RateLimits::default());
        library.set_rate_limits(limits).unwrap();
    }
    let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
    assert_eq!(library.rate_limits().unwrap(), limits);
    library.set_rate_limits(RateLimits::default()).unwrap();
    assert_eq!(library.rate_limits().unwrap(), RateLimits::default());
}

#[test]
fn progress_follows_opening_watching_and_manual_marks_across_restarts() {
    use learning_application::LessonStatus;

    let root = TempRoot::new();
    let course_root = create_course(root.path());
    let database = root.path().join("library.sqlite3");
    let status = |library: &CourseLibrary| {
        let view = library.course_progress("course_recovery").unwrap().unwrap();
        (
            view.lessons[0].1,
            view.summary.completed,
            view.summary.total,
        )
    };
    {
        let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
        library
            .import_local_folder(&course_root, &CancellationFlag::default())
            .unwrap();
        assert!(library.course_progress("course_missing").unwrap().is_none());
        assert_eq!(status(&library), (LessonStatus::NotStarted, 0, 1));
        assert!(
            !library
                .record_resource_opened("course_recovery", "missing")
                .unwrap()
        );
        assert!(
            library
                .record_resource_opened("course_recovery", "resource_video")
                .unwrap()
        );
        assert_eq!(status(&library), (LessonStatus::InProgress, 0, 1));
        library
            .save_playback_position("course_recovery", "resource_video", 120.0, 600.0)
            .unwrap();
        assert_eq!(status(&library).0, LessonStatus::InProgress);
        library
            .save_playback_position("course_recovery", "resource_video", 560.0, 600.0)
            .unwrap();
        assert_eq!(status(&library), (LessonStatus::Completed, 1, 1));
    }
    let mut library = CourseLibrary::open(&database, ImportLimits::default()).unwrap();
    assert_eq!(status(&library), (LessonStatus::Completed, 1, 1));
    assert!(
        !library
            .set_lesson_completed("course_recovery", "lesson_missing", true)
            .unwrap()
    );
    assert!(
        library
            .set_lesson_completed("course_recovery", "lesson_one", false)
            .unwrap()
    );
    assert_eq!(status(&library), (LessonStatus::InProgress, 0, 1));
    assert!(
        library
            .set_lesson_completed("course_recovery", "lesson_one", true)
            .unwrap()
    );
    assert_eq!(status(&library).0, LessonStatus::Completed);
}

fn notes_fixture() -> (TempRoot, CourseLibrary) {
    let root = TempRoot::new();
    let course_root = create_course(root.path());
    let mut library = CourseLibrary::open(
        &root.path().join("library.sqlite3"),
        ImportLimits::default(),
    )
    .unwrap();
    library
        .import_local_folder(&course_root, &CancellationFlag::default())
        .unwrap();
    (root, library)
}

fn note_doc(text: &str) -> String {
    json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
        .to_string()
}

fn video_at(seconds: f64) -> learning_application::Anchor {
    learning_application::Anchor::VideoTime {
        resource_id: "resource_video".to_owned(),
        seconds,
    }
}

#[test]
fn notes_are_anchored_only_to_real_targets() {
    use learning_application::Anchor;

    let (_root, mut library) = notes_fixture();
    let (body, at) = (note_doc, video_at);
    let note = library
        .create_note(
            "course_recovery",
            "lesson_one",
            &at(75.0),
            &body("Caching tiers"),
        )
        .unwrap();
    assert_eq!((note.revision, note.text.as_str()), (0, "Caching tiers"));
    library
        .create_note(
            "course_recovery",
            "lesson_one",
            &Anchor::Lesson {},
            &body("Overview"),
        )
        .unwrap();
    for (lesson, anchor) in [
        ("lesson_missing", Anchor::Lesson {}),
        (
            "lesson_one",
            Anchor::PdfPage {
                resource_id: "resource_video".to_owned(),
                page: 1,
                region: None,
            },
        ),
        (
            "lesson_one",
            Anchor::VideoTime {
                resource_id: "resource_other".to_owned(),
                seconds: 1.0,
            },
        ),
    ] {
        assert!(matches!(
            library.create_note("course_recovery", lesson, &anchor, &body("x")),
            Err(LibraryError::NoteTarget(_))
        ));
    }
    assert!(matches!(
        library.create_note("course_recovery", "lesson_one", &at(-1.0), &body("x")),
        Err(LibraryError::Note(_))
    ));
    assert!(matches!(
        library.create_note("course_recovery", "lesson_one", &at(1.0), "{}"),
        Err(LibraryError::Note(_))
    ));
}

#[test]
fn notes_change_only_from_their_current_revision() {
    use learning_application::NoteChange;

    let (_root, mut library) = notes_fixture();
    let (body, at) = (note_doc, video_at);
    let note = library
        .create_note(
            "course_recovery",
            "lesson_one",
            &at(75.0),
            &body("Caching tiers"),
        )
        .unwrap();
    library
        .create_note(
            "course_recovery",
            "lesson_one",
            &learning_application::Anchor::Lesson {},
            &body("Overview"),
        )
        .unwrap();
    let NoteChange::Saved(updated) = library
        .update_note(&note.note_id, 0, &at(80.0), &body("Caching tiers and TTLs"))
        .unwrap()
    else {
        panic!("saved");
    };
    assert_eq!(updated.revision, 1);
    assert_eq!(
        library
            .update_note(&note.note_id, 0, &at(90.0), &body("stale edit"))
            .unwrap(),
        NoteChange::Conflict {
            current_revision: 1
        }
    );
    let notes = library
        .notes("course_recovery", Some("lesson_one"))
        .unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].text, "Caching tiers and TTLs");
    assert_eq!(notes[0].anchor, at(80.0));
    assert_eq!(
        library.delete_note(&note.note_id, 0).unwrap(),
        NoteChange::Conflict {
            current_revision: 1
        }
    );
    assert!(matches!(
        library.delete_note(&note.note_id, 1).unwrap(),
        NoteChange::Saved(_)
    ));
    assert_eq!(
        library
            .update_note("note_missing", 0, &at(1.0), &body("x"))
            .unwrap(),
        NoteChange::NotFound
    );
    assert_eq!(library.notes("course_recovery", None).unwrap().len(), 1);
}

#[test]
fn pdf_documents_are_read_only_while_they_match_the_course() {
    use learning_application::{
        DocumentError, DocumentSource, MAX_DOCUMENT_BYTES, read_local_document,
    };

    let root = TempRoot::new();
    let course = root.path().join("documents");
    fs::create_dir(&course).unwrap();
    let pdf = b"%PDF-1.4\n% tiny test document\n";
    fs::write(course.join("handout.pdf"), pdf).unwrap();
    let digest = Sha256::digest(pdf)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(&mut hex, "{byte:02x}").unwrap();
            hex
        });
    let manifest = json!({
        "schemaVersion": 1, "courseId": "course_documents", "version": "1.0.0", "title": "Documents",
        "publisher": { "name": "Zeron Tests" }, "language": "en",
        "modules": [{ "id": "module_one", "title": "Module", "position": 0, "lessons": [{
            "id": "lesson_one", "title": "Lesson", "position": 0,
            "resources": [{ "id": "resource_pdf", "kind": "pdf", "path": "handout.pdf", "mediaType": "application/pdf", "byteLength": pdf.len(), "sha256": digest }]
        }] }]
    });
    fs::write(
        course.join("course.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut library = CourseLibrary::open(
        &root.path().join("library.sqlite3"),
        ImportLimits::default(),
    )
    .unwrap();
    library
        .import_local_folder(&course, &CancellationFlag::default())
        .unwrap();

    assert!(
        library
            .document_source("course_documents", "resource_missing")
            .unwrap()
            .is_none()
    );
    assert!(
        library
            .document_source("course_missing", "resource_pdf")
            .unwrap()
            .is_none()
    );
    let Some(DocumentSource::Local {
        path,
        sha256,
        byte_length,
    }) = library
        .document_source("course_documents", "resource_pdf")
        .unwrap()
    else {
        panic!("a local document");
    };
    assert_eq!(
        read_local_document(&path, &sha256, byte_length).unwrap(),
        pdf
    );
    assert!(matches!(
        read_local_document(&path, &sha256, MAX_DOCUMENT_BYTES + 1),
        Err(DocumentError::TooLarge)
    ));
    fs::write(&path, b"%PDF-1.4\n% changed after import\n").unwrap();
    assert!(matches!(
        read_local_document(&path, &sha256, byte_length),
        Err(DocumentError::Changed)
    ));
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        read_local_document(&path, &sha256, byte_length),
        Err(DocumentError::Unavailable)
    ));

    // Video resources are not documents.
    let video_root = create_course(root.path());
    library
        .import_local_folder(&video_root, &CancellationFlag::default())
        .unwrap();
    assert!(
        library
            .document_source("course_recovery", "resource_video")
            .unwrap()
            .is_none()
    );
}

#[test]
fn drawings_save_from_their_current_revision_and_keep_images() {
    use learning_application::{DrawingFile, DrawingSave, LibraryError};

    let (_root, mut library) = notes_fixture();
    let scene = |label: &str| json!({ "elements": [], "appState": { "name": label } }).to_string();
    let png = |id: &str, data: &str| DrawingFile {
        id: id.to_owned(),
        mime_type: "image/png".to_owned(),
        data_url: format!("data:image/png;base64,{data}"),
    };
    assert_eq!(
        library.drawing("course_recovery", "lesson_one").unwrap(),
        None
    );
    assert_eq!(
        library
            .save_drawing(
                "course_recovery",
                "lesson_one",
                None,
                &scene("a"),
                &[png("img", "AAAA")]
            )
            .unwrap(),
        DrawingSave::Saved { revision: 0 }
    );
    assert_eq!(
        library
            .save_drawing(
                "course_recovery",
                "lesson_one",
                Some(0),
                &scene("b"),
                &[png("img", "BBBB"), png("img2", "CCCC")],
            )
            .unwrap(),
        DrawingSave::Saved { revision: 1 }
    );
    assert_eq!(
        library
            .save_drawing(
                "course_recovery",
                "lesson_one",
                Some(0),
                &scene("stale"),
                &[]
            )
            .unwrap(),
        DrawingSave::Conflict {
            current_revision: 1
        }
    );
    let drawing = library
        .drawing("course_recovery", "lesson_one")
        .unwrap()
        .unwrap();
    assert_eq!((drawing.scene_json, drawing.revision), (scene("b"), 1));
    assert_eq!(drawing.files, [png("img", "AAAA"), png("img2", "CCCC")]);

    for (lesson, scene_json, files) in [
        ("lesson_missing", scene("x"), vec![]),
        ("lesson_one", "{}".to_owned(), vec![]),
        (
            "lesson_one",
            scene("x"),
            vec![DrawingFile {
                id: "svg".to_owned(),
                mime_type: "image/svg+xml".to_owned(),
                data_url: "data:image/svg+xml;base64,AAAA".to_owned(),
            }],
        ),
    ] {
        assert!(matches!(
            library.save_drawing("course_recovery", lesson, Some(1), &scene_json, &files),
            Err(LibraryError::NoteTarget(_) | LibraryError::Drawing(_))
        ));
    }
    let many: Vec<_> = (0..64)
        .map(|index| png(&format!("f{index}"), "AAAA"))
        .collect();
    assert!(matches!(
        library.save_drawing("course_recovery", "lesson_one", Some(1), &scene("x"), &many),
        Err(LibraryError::Drawing(_))
    ));
    assert_eq!(
        library
            .drawing("course_recovery", "lesson_one")
            .unwrap()
            .unwrap()
            .revision,
        1
    );
}

const TRANSCRIPT_VTT: &[u8] = b"WEBVTT\n\n00:01.000 --> 00:02.000\nHello\n";

fn transcript_fixture() -> (TempRoot, CourseLibrary) {
    let vtt = TRANSCRIPT_VTT;
    let root = TempRoot::new();
    let course = root.path().join("transcripts");
    fs::create_dir(&course).unwrap();
    let mut resources = Vec::new();
    let mut add = |id: &str, kind: &str, file: &str, bytes: &[u8], extra: serde_json::Value| {
        fs::write(course.join(file), bytes).unwrap();
        let digest = Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(&mut hex, "{byte:02x}").unwrap();
                hex
            });
        let mut resource = json!({ "id": id, "kind": kind, "path": file, "mediaType": "application/octet-stream", "byteLength": bytes.len(), "sha256": digest });
        for (key, value) in extra.as_object().unwrap() {
            resource[key] = value.clone();
        }
        resources.push(resource);
    };
    add(
        "video_a",
        "video",
        "a.mp4",
        b"video a",
        json!({ "relations": [{ "kind": "subtitle", "resourceId": "subs_fr" }] }),
    );
    add("video_b", "video", "b.mp4", b"video b", json!({}));
    add(
        "subs_fr",
        "subtitle",
        "a.fr.vtt",
        vtt,
        json!({ "language": "fr" }),
    );
    add(
        "transcript_en",
        "transcript",
        "en.vtt",
        vtt,
        json!({ "language": "en" }),
    );
    add("handout", "pdf", "handout.pdf", b"%PDF-1.4\n", json!({}));
    add("readme", "markdown", "README.md", b"# Notes\n", json!({}));
    add("plain", "text", "notes.txt", b"plain notes\n", json!({}));
    let manifest = json!({
        "schemaVersion": 1, "courseId": "course_transcripts", "version": "1.0.0", "title": "Transcripts",
        "publisher": { "name": "Zeron Tests" }, "language": "en",
        "modules": [{ "id": "module_one", "title": "Module", "position": 0, "lessons": [
            { "id": "lesson_one", "title": "Lesson", "position": 0, "resources": resources },
        ] }]
    });
    fs::write(
        course.join("course.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut library = CourseLibrary::open(
        &root.path().join("library.sqlite3"),
        ImportLimits::default(),
    )
    .unwrap();
    library
        .import_local_folder(&course, &CancellationFlag::default())
        .unwrap();

    (root, library)
}

#[test]
fn media_finds_its_transcript_by_relation_then_by_lesson() {
    use learning_application::{DocumentSource, read_local_document};

    let (_root, library) = transcript_fixture();
    let vtt = TRANSCRIPT_VTT;
    let (id, language, source) = library
        .transcript_source("course_transcripts", "video_a")
        .unwrap()
        .unwrap();
    assert_eq!((id.as_str(), language.as_deref()), ("subs_fr", Some("fr")));
    let DocumentSource::Local {
        path,
        sha256,
        byte_length,
    } = source
    else {
        panic!("a local transcript");
    };
    assert_eq!(
        read_local_document(&path, &sha256, byte_length).unwrap(),
        vtt
    );
    let (id, language, _) = library
        .transcript_source("course_transcripts", "video_b")
        .unwrap()
        .unwrap();
    assert_eq!(
        (id.as_str(), language.as_deref()),
        ("transcript_en", Some("en"))
    );
    for (course_id, resource) in [
        ("course_transcripts", "handout"),
        ("course_transcripts", "transcript_en"),
        ("course_transcripts", "missing"),
        ("course_missing", "video_a"),
    ] {
        assert!(
            library
                .transcript_source(course_id, resource)
                .unwrap()
                .is_none(),
            "{resource}"
        );
    }
    assert!(
        library
            .document_source("course_transcripts", "transcript_en")
            .unwrap()
            .is_none()
    );
}

#[test]
fn markdown_and_text_resources_are_readable_text() {
    use learning_application::{DocumentSource, read_local_document};

    let (_root, library) = transcript_fixture();
    let (markdown, source) = library
        .text_source("course_transcripts", "readme")
        .unwrap()
        .unwrap();
    let DocumentSource::Local {
        path,
        sha256,
        byte_length,
    } = source
    else {
        panic!("a local text resource");
    };
    assert!(markdown);
    assert_eq!(
        read_local_document(&path, &sha256, byte_length).unwrap(),
        b"# Notes\n"
    );
    let (markdown, source) = library
        .text_source("course_transcripts", "plain")
        .unwrap()
        .unwrap();
    assert!(!markdown);
    assert_eq!(source.byte_length(), 12);
    for resource in ["handout", "transcript_en", "video_a", "missing"] {
        assert!(
            library
                .text_source("course_transcripts", resource)
                .unwrap()
                .is_none(),
            "{resource}"
        );
    }
}
