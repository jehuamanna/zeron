//! Moving annotations to a course version that renamed, replaced, or removed
//! lessons.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use learning_application::{
    Anchor, AnnotationMove, CancellationFlag, CourseLibrary, ImportLimits, LibraryError,
    MatchReason,
};
use serde_json::json;
use sha2::{Digest, Sha256};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-migration-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `(lesson id, title, [(resource id, kind, file, bytes)])`
type LessonSpec<'a> = (
    &'a str,
    &'a str,
    &'a [(&'a str, &'a str, &'a str, &'a [u8])],
);

fn package(parent: &Path, version: &str, lessons: &[LessonSpec<'_>]) -> PathBuf {
    let root = parent.join(version);
    fs::create_dir_all(&root).unwrap();
    let lessons: Vec<_> = lessons
        .iter()
        .enumerate()
        .map(|(position, (id, title, resources))| {
            let resources: Vec<_> = resources
                .iter()
                .map(|(resource_id, kind, file, bytes)| {
                    fs::write(root.join(file), bytes).unwrap();
                    let mut digest = String::new();
                    for byte in Sha256::digest(bytes) {
                        write!(&mut digest, "{byte:02x}").unwrap();
                    }
                    let media_type = if *kind == "pdf" { "application/pdf" } else { "video/mp4" };
                    json!({ "id": resource_id, "kind": kind, "path": file, "mediaType": media_type, "byteLength": bytes.len(), "sha256": digest })
                })
                .collect();
            json!({ "id": id, "title": title, "position": position, "resources": resources })
        })
        .collect();
    let manifest = json!({
        "schemaVersion": 1,
        "courseId": "course_migrate",
        "version": version,
        "title": "Systems",
        "publisher": { "name": "Zeron Tests" },
        "language": "en",
        "modules": [{ "id": "module", "title": "Module", "position": 0, "lessons": lessons }]
    });
    fs::write(
        root.join("course.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    root
}

fn doc(text: &str) -> String {
    json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
        .to_string()
}

const COURSE: &str = "course_migrate";

fn video(resource_id: &str, seconds: f64) -> Anchor {
    Anchor::VideoTime {
        resource_id: resource_id.to_owned(),
        seconds,
    }
}

fn find<'a>(moves: &'a [AnnotationMove], lesson_id: &str) -> &'a AnnotationMove {
    moves
        .iter()
        .find(|item| item.from_lesson_id == lesson_id)
        .unwrap()
}

/// Four lessons: Intro, Caching (slides and video), Old topic, and Kept.
fn version_one(parent: &Path) -> PathBuf {
    package(
        parent,
        "1.0.0",
        &[
            (
                "lesson_a",
                "Intro",
                &[("video_a", "video", "a.mp4", b"intro video")],
            ),
            (
                "lesson_b",
                "Caching",
                &[
                    ("pdf_b", "pdf", "b.pdf", b"%PDF caching slides"),
                    ("video_b", "video", "b.mp4", b"caching video"),
                ],
            ),
            (
                "lesson_c",
                "Old topic",
                &[("video_c", "video", "c.mp4", b"old topic")],
            ),
            (
                "lesson_k",
                "Kept",
                &[("video_k", "video", "k.mp4", b"kept one")],
            ),
        ],
    )
}

/// Intro renamed with the same video, Caching re-recorded, Old topic removed,
/// and Kept with a new video.
fn version_two(parent: &Path) -> PathBuf {
    package(
        parent,
        "1.1.0",
        &[
            (
                "lesson_intro",
                "Introduction",
                &[("v_intro", "video", "intro.mp4", b"intro video")],
            ),
            (
                "lesson_cache",
                "  caching ",
                &[(
                    "v_cache",
                    "video",
                    "cache.mp4",
                    b"caching video, re-recorded",
                )],
            ),
            (
                "lesson_k",
                "Kept",
                &[("video_k2", "video", "k2.mp4", b"kept two")],
            ),
        ],
    )
}

/// Notes on every lesson, a drawing on Intro, completed Caching, and a
/// playback position in Intro's video.
fn annotate(library: &mut CourseLibrary) {
    library
        .create_note(
            COURSE,
            "lesson_a",
            &video("video_a", 12.0),
            &doc("intro at 12s"),
        )
        .unwrap();
    library
        .create_note(
            COURSE,
            "lesson_b",
            &Anchor::PdfPage {
                resource_id: "pdf_b".to_owned(),
                page: 2,
                region: None,
            },
            &doc("slide two"),
        )
        .unwrap();
    library
        .create_note(
            COURSE,
            "lesson_b",
            &video("video_b", 30.0),
            &doc("cache at 30s"),
        )
        .unwrap();
    library
        .create_note(
            COURSE,
            "lesson_c",
            &Anchor::Lesson {},
            &doc("old topic note"),
        )
        .unwrap();
    library
        .create_note(
            COURSE,
            "lesson_k",
            &video("video_k", 5.0),
            &doc("kept at 5s"),
        )
        .unwrap();
    library
        .save_drawing(
            COURSE,
            "lesson_a",
            None,
            &json!({ "elements": [] }).to_string(),
            &[],
        )
        .unwrap();
    library
        .set_lesson_completed(COURSE, "lesson_b", true)
        .unwrap();
    assert!(
        library
            .save_playback_position(COURSE, "video_a", 40.0, 100.0)
            .unwrap()
    );
}

/// Version 1.0.0 with notes, a drawing, progress, and a position, replaced by
/// version 1.1.0, which renames, re-records, removes, and changes lessons.
fn updated_library(name: &str) -> (TempRoot, CourseLibrary) {
    let temp = TempRoot::new(name);
    let v1 = version_one(&temp.0);
    let mut library =
        CourseLibrary::open(&temp.0.join("library.sqlite3"), ImportLimits::default()).unwrap();
    library
        .import_local_folder(&v1, &CancellationFlag::default())
        .unwrap();
    annotate(&mut library);
    assert_eq!(library.annotation_moves(COURSE).unwrap().len(), 0);

    let v2 = version_two(&temp.0);
    library
        .import_local_folder(&v2, &CancellationFlag::default())
        .unwrap();
    (temp, library)
}

#[test]
fn suggestions_follow_identical_files_then_titles() {
    let (_temp, library) = updated_library("suggest");
    let moves = library.annotation_moves(COURSE).unwrap();
    assert_eq!(moves.len(), 4);
    let a = find(&moves, "lesson_a");
    assert_eq!(a.suggested_lesson_id.as_deref(), Some("lesson_intro"));
    assert_eq!(a.reason, Some(MatchReason::SameContent));
    assert_eq!(
        (a.notes, a.notes_widened, a.has_drawing, a.has_progress),
        (1, 0, true, false)
    );
    assert_eq!(a.from_title.as_deref(), Some("Intro"));
    assert_eq!(a.from_version.as_deref(), Some("1.0.0"));
    let b = find(&moves, "lesson_b");
    assert_eq!(b.suggested_lesson_id.as_deref(), Some("lesson_cache"));
    assert_eq!(b.reason, Some(MatchReason::SameTitle));
    assert_eq!(
        (b.notes, b.notes_widened, b.has_drawing, b.has_progress),
        (2, 2, false, true)
    );
    let c = find(&moves, "lesson_c");
    assert_eq!(
        (c.suggested_lesson_id.as_deref(), c.reason, c.notes),
        (None, None, 1)
    );
    let k = find(&moves, "lesson_k");
    assert_eq!(k.suggested_lesson_id.as_deref(), Some("lesson_k"));
    assert_eq!(k.reason, Some(MatchReason::SameLesson));
    assert_eq!(k.notes_widened, 1);
    assert!(matches!(
        library.annotation_moves("course_unknown"),
        Err(LibraryError::NoteTarget(_))
    ));
}

#[test]
fn moves_carry_anchors_drawings_progress_and_positions() {
    let (_temp, mut library) = updated_library("move");
    let intro = library
        .move_annotations(COURSE, "lesson_a", "lesson_intro")
        .unwrap();
    assert_eq!(
        (
            intro.notes_moved,
            intro.notes_widened,
            intro.drawing_moved,
            intro.positions_moved
        ),
        (1, 0, true, 1)
    );
    let notes = library.notes(COURSE, Some("lesson_intro")).unwrap();
    assert_eq!(notes[0].anchor, video("v_intro", 12.0));
    assert_eq!(notes[0].text, "intro at 12s");
    assert!(library.drawing(COURSE, "lesson_intro").unwrap().is_some());
    assert_eq!(
        library.playback_position(COURSE, "v_intro").unwrap(),
        Some(40.0)
    );

    let cache = library
        .move_annotations(COURSE, "lesson_b", "lesson_cache")
        .unwrap();
    assert_eq!(
        (cache.notes_moved, cache.notes_widened, cache.progress_moved),
        (2, 2, true)
    );
    assert!(
        library
            .notes(COURSE, Some("lesson_cache"))
            .unwrap()
            .iter()
            .all(|note| note.anchor == Anchor::Lesson {})
    );
    let kept = library
        .move_annotations(COURSE, "lesson_k", "lesson_k")
        .unwrap();
    assert_eq!((kept.notes_moved, kept.notes_widened), (1, 1));

    let remaining = library.annotation_moves(COURSE).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].from_lesson_id, "lesson_c");
    assert!(matches!(
        library.move_annotations(COURSE, "lesson_c", "lesson_missing"),
        Err(LibraryError::NoteTarget(_))
    ));
    let chosen = library
        .move_annotations(COURSE, "lesson_c", "lesson_intro")
        .unwrap();
    assert_eq!(
        (
            chosen.notes_moved,
            chosen.drawing_moved,
            chosen.drawing_kept
        ),
        (1, false, false)
    );
    assert_eq!(
        library.notes(COURSE, Some("lesson_intro")).unwrap().len(),
        2
    );
    assert_eq!(library.annotation_moves(COURSE).unwrap().len(), 0);
}

#[test]
fn a_drawing_never_replaces_the_target_lessons_own() {
    let temp = TempRoot::new("drawing");
    let v1 = package(
        &temp.0,
        "1.0.0",
        &[
            (
                "lesson_old",
                "Topic",
                &[("r", "video", "a.mp4", b"same bytes")],
            ),
            ("lesson_x", "Other", &[]),
        ],
    );
    let mut library =
        CourseLibrary::open(&temp.0.join("library.sqlite3"), ImportLimits::default()).unwrap();
    library
        .import_local_folder(&v1, &CancellationFlag::default())
        .unwrap();
    let scene = |label: &str| json!({ "elements": [], "appState": { "name": label } }).to_string();
    library
        .save_drawing(COURSE, "lesson_old", None, &scene("old"), &[])
        .unwrap();
    let v2 = package(
        &temp.0,
        "2.0.0",
        &[
            (
                "lesson_new",
                "Topic",
                &[("r2", "video", "b.mp4", b"same bytes")],
            ),
            ("lesson_x", "Other", &[]),
        ],
    );
    library
        .import_local_folder(&v2, &CancellationFlag::default())
        .unwrap();
    library
        .save_drawing(COURSE, "lesson_new", None, &scene("new"), &[])
        .unwrap();

    let moved = library
        .move_annotations(COURSE, "lesson_old", "lesson_new")
        .unwrap();
    assert!(moved.drawing_kept && !moved.drawing_moved);
    assert!(
        library
            .drawing(COURSE, "lesson_new")
            .unwrap()
            .unwrap()
            .scene_json
            .contains("\"new\"")
    );
    assert!(
        library
            .drawing(COURSE, "lesson_old")
            .unwrap()
            .unwrap()
            .scene_json
            .contains("\"old\"")
    );
    assert_eq!(library.annotation_moves(COURSE).unwrap().len(), 1);
}
