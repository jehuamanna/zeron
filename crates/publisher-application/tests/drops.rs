//! Dropped files and folders become draft structure.

use std::{cmp::Ordering, fs, path::PathBuf, time::Duration};

use course_format::ResourceKind;
use publisher_application::{
    DraftServiceError, DropInbox, DropLimits, PublisherDrafts, SkipReason, StructureEdit,
    clean_title, natural_cmp, plan_drop,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-app-drops-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn skipped(plan: &publisher_application::DropPlan) -> Vec<(String, SkipReason)> {
    plan.skipped
        .iter()
        .map(|entry| (entry.name.clone(), entry.reason))
        .collect()
}

#[test]
fn names_sort_naturally_and_titles_lose_extensions_and_ordinals() {
    let mut names = vec!["lesson10", "lesson2", "Lesson1", "lesson20", "lesson02b"];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(
        names,
        ["Lesson1", "lesson2", "lesson02b", "lesson10", "lesson20"]
    );
    let mut designs = [
        "10 Design Tinder.mp4.mp4",
        "2 Design AlgoExpert.mp4",
        "1 Design A.mp4",
    ];
    designs.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(designs[0], "1 Design A.mp4");
    assert_eq!(designs[2], "10 Design Tinder.mp4.mp4");
    assert_eq!(natural_cmp("a", "a"), Ordering::Equal);
    assert_eq!(natural_cmp("a1", "a01"), "a1".cmp("a01"));

    assert_eq!(
        clean_title("10 Design Tinder.mp4.mp4", true),
        "Design Tinder"
    );
    assert_eq!(
        clean_title("1 Design A Code-Deployment System.mp4", true),
        "Design A Code-Deployment System"
    );
    assert_eq!(clean_title("lesson1.mp4", true), "lesson1");
    assert_eq!(clean_title("01 - Intro.PDF", true), "Intro");
    assert_eq!(clean_title("notes.v2.pdf", true), "notes.v2");
    assert_eq!(clean_title("42.mp4", true), "42");
    assert_eq!(clean_title("Course.v1", false), "Course.v1");
    assert_eq!(clean_title("  ", true), "Untitled");
    assert_eq!(clean_title("a\u{0}b.mp4", true), "a b");
    assert_eq!(clean_title(&"x".repeat(500), true).chars().count(), 200);
}

#[cfg(unix)]
#[test]
fn folders_become_modules_and_everything_left_out_is_reported() {
    let scratch = Scratch::new("plan");
    for name in [
        "lesson10.mp4",
        "lesson2.mp4",
        "lesson1.mp4",
        "slides.pdf",
        "talk.m4a",
    ] {
        scratch.file(&format!("Fundamentals/{name}"), b"x");
    }
    scratch.file("Fundamentals/Design Fundamentals.txt", b"titles");
    scratch.file("Fundamentals/.DS_Store.mp4", b"x");
    scratch.file("Fundamentals/extra/deep.mp4", b"x");
    std::os::unix::fs::symlink(
        scratch.0.join("Fundamentals/lesson1.mp4"),
        scratch.0.join("Fundamentals/link.mp4"),
    )
    .unwrap();
    scratch.file("Notes Only/readme.md", b"x");
    scratch.file(".hidden/a.mp4", b"x");
    let loose = scratch.file("handout.pdf", b"x");
    std::os::unix::fs::symlink(&loose, scratch.0.join("handout-link.pdf")).unwrap();

    let dropped = [
        scratch.0.join("Notes Only"),
        scratch.0.join("handout.pdf"),
        scratch.0.join("Fundamentals"),
        scratch.0.join(".hidden"),
        scratch.0.join("handout-link.pdf"),
        scratch.0.join("missing.mp4"),
    ];
    let plan = plan_drop(&dropped, DropLimits::default());
    assert_eq!(plan.modules.len(), 1);
    let module = &plan.modules[0];
    assert_eq!(module.title, "Fundamentals");
    let lessons: Vec<_> = module
        .lessons
        .iter()
        .map(|lesson| lesson.title.as_str())
        .collect();
    assert_eq!(
        lessons,
        ["lesson1", "lesson2", "lesson10", "slides", "talk"]
    );
    assert_eq!(module.lessons[3].kind, ResourceKind::Pdf);
    assert_eq!(module.lessons[4].kind, ResourceKind::Audio);
    assert_eq!(plan.loose_files.len(), 1);
    assert_eq!(plan.loose_files[0].file_name, "handout.pdf");

    let mut reported = skipped(&plan);
    reported.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected = vec![
        (".hidden".to_owned(), SkipReason::Hidden),
        ("Fundamentals/.DS_Store.mp4".to_owned(), SkipReason::Hidden),
        (
            "Fundamentals/Design Fundamentals.txt".to_owned(),
            SkipReason::UnsupportedType,
        ),
        ("Fundamentals/extra".to_owned(), SkipReason::NestedFolder),
        ("Fundamentals/link.mp4".to_owned(), SkipReason::SymbolicLink),
        (
            "Notes Only/readme.md".to_owned(),
            SkipReason::UnsupportedType,
        ),
        ("Notes Only".to_owned(), SkipReason::NoLessons),
        ("handout-link.pdf".to_owned(), SkipReason::SymbolicLink),
        ("missing.mp4".to_owned(), SkipReason::Unreadable),
    ];
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(reported, expected);
    for entry in &plan.skipped {
        assert!(!entry.name.contains(&*scratch.0.to_string_lossy()));
    }
}

#[test]
fn drops_are_bounded() {
    let scratch = Scratch::new("limits");
    for index in 0..6 {
        scratch.file(&format!("A/{index}.mp4"), b"x");
        scratch.file(&format!("B/{index}.mp4"), b"x");
    }
    let limits = DropLimits {
        max_entries: 100,
        max_files: 4,
        max_folders: 1,
    };
    let plan = plan_drop(&[scratch.0.join("A"), scratch.0.join("B")], limits);
    assert_eq!(plan.modules.len(), 1);
    assert_eq!(plan.modules[0].lessons.len(), 4);
    assert!(
        plan.skipped
            .iter()
            .any(|entry| entry.name == "B" && entry.reason == SkipReason::LimitReached)
    );
    assert_eq!(
        plan.skipped
            .iter()
            .filter(|entry| entry.reason == SkipReason::LimitReached)
            .count(),
        3
    );

    let tight = DropLimits {
        max_entries: 3,
        ..DropLimits::default()
    };
    let plan = plan_drop(&[scratch.0.join("A")], tight);
    assert!(plan.skipped.iter().any(|entry| entry.name == "A/…"));
    assert!(plan.modules[0].lessons.len() <= 2);
}

fn draft_with_lesson(drafts: &mut PublisherDrafts) -> (String, String) {
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let module_id = drafts
        .edit_structure(
            &draft_id,
            0,
            StructureEdit::AddModule {
                title: "Existing".to_owned(),
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
                title: "Welcome".to_owned(),
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
fn applying_a_drop_is_one_revision_and_skips_vanished_files() {
    let scratch = Scratch::new("apply");
    scratch.file("Module A/2 Second.mp4", b"x");
    scratch.file("Module A/1 First.pdf", b"%PDF-1.7");
    let vanishing = scratch.file("Module B/gone.mp4", b"x");
    let loose = scratch.file("handout.pdf", b"%PDF-1.7");
    let plan = plan_drop(
        &[
            scratch.0.join("Module A"),
            scratch.0.join("Module B"),
            loose,
        ],
        DropLimits::default(),
    );
    fs::remove_file(vanishing).unwrap();

    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);
    assert!(matches!(
        drafts.apply_drop(&draft_id, 1, &plan, Some(&lesson_id)),
        Err(DraftServiceError::RevisionConflict { .. })
    ));
    assert!(matches!(
        drafts.apply_drop(&draft_id, 2, &plan, Some("lesson_missing")),
        Err(DraftServiceError::EntityNotFound { .. })
    ));
    assert_eq!(drafts.draft(&draft_id).unwrap().unwrap().draft.revision, 2);

    let outcome = drafts
        .apply_drop(&draft_id, 2, &plan, Some(&lesson_id))
        .unwrap();
    assert_eq!(outcome.draft.draft.revision, 3);
    assert_eq!(
        (
            outcome.modules_added,
            outcome.lessons_added,
            outcome.files_attached
        ),
        (1, 2, 1)
    );
    assert_eq!(
        outcome
            .skipped
            .iter()
            .map(|entry| (entry.name.as_str(), entry.reason))
            .collect::<Vec<_>>(),
        [("Module B/gone.mp4", SkipReason::Unreadable)]
    );
    let draft = drafts.draft(&draft_id).unwrap().unwrap().draft;
    assert_eq!(draft.modules.len(), 2);
    let imported = &draft.modules[1];
    assert_eq!(imported.title, "Module A");
    let titles: Vec<_> = imported
        .lessons
        .iter()
        .map(|lesson| lesson.title.as_str())
        .collect();
    assert_eq!(titles, ["First", "Second"]);
    assert!(
        imported
            .lessons
            .iter()
            .all(|lesson| lesson.resources.len() == 1)
    );
    assert_eq!(imported.lessons[0].resources[0].kind, ResourceKind::Pdf);
    assert!(
        imported.lessons[1].resources[0]
            .package_path
            .ends_with("/2_Second.mp4")
    );
    assert!(imported.lessons[1].resources[0].source_path.is_absolute());
    assert_eq!(draft.modules[0].lessons[0].resources.len(), 1);
    assert_eq!(
        draft.modules[0].lessons[0].resources[0].media_type,
        "application/pdf"
    );
}

#[test]
fn loose_files_become_one_module_or_attach_to_a_chosen_lesson() {
    let scratch = Scratch::new("loose");
    let files = [
        scratch.file("Week 1/02 - Caching.mp4", b"x"),
        scratch.file("Week 1/01 - Intro.mp4", b"x"),
        scratch.file("Week 1/handout.pdf", b"%PDF-1.7"),
        scratch.file("Week 1/notes.txt", b"x"),
    ];
    let plan = plan_drop(&files, DropLimits::default());
    assert_eq!(plan.loose_module_title, "Week 1");
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let (draft_id, lesson_id) = draft_with_lesson(&mut drafts);

    // Without a lesson, the files become the lessons of one new module.
    let outcome = drafts.apply_drop(&draft_id, 2, &plan, None).unwrap();
    assert_eq!(
        (
            outcome.modules_added,
            outcome.lessons_added,
            outcome.files_attached
        ),
        (1, 3, 0)
    );
    let module = outcome.draft.draft.modules.last().unwrap();
    assert_eq!(module.title, "Week 1");
    let titles: Vec<_> = module
        .lessons
        .iter()
        .map(|lesson| lesson.title.as_str())
        .collect();
    assert_eq!(titles, ["Intro", "Caching", "handout"]);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].name, "notes.txt");
    assert_eq!(outcome.skipped[0].reason, SkipReason::UnsupportedType);

    // With a lesson, every file attaches to it, as before.
    let attached = drafts
        .apply_drop(&draft_id, 3, &plan, Some(&lesson_id))
        .unwrap();
    assert_eq!((attached.modules_added, attached.files_attached), (0, 4));

    // Files from different folders get a generic module title.
    let other = scratch.file("Elsewhere/extra.mp4", b"x");
    let mixed = plan_drop(&[files[0].clone(), other], DropLimits::default());
    assert_eq!(mixed.loose_module_title, "New module");

    let empty = plan_drop(&[scratch.0.join("missing")], DropLimits::default());
    assert!(empty.is_empty());
    assert!(matches!(
        drafts.apply_drop(&draft_id, 4, &empty, None),
        Err(DraftServiceError::InvalidInput { ref field, .. }) if field == "drop"
    ));
    assert_eq!(drafts.draft(&draft_id).unwrap().unwrap().draft.revision, 4);
}

#[test]
fn a_course_folder_turns_its_folders_into_modules() {
    let scratch = Scratch::new("course");
    scratch.file("Course/Module 10/a.mp4", b"x");
    scratch.file("Course/Module 2/b.mp4", b"x");
    scratch.file("Course/Module 2/deeper/c.mp4", b"x");
    scratch.file("Course/Empty/readme.md", b"x");
    scratch.file("Course/syllabus.txt", b"x");
    let plan = plan_drop(&[scratch.0.join("Course")], DropLimits::default());
    let modules: Vec<_> = plan
        .modules
        .iter()
        .map(|module| module.title.as_str())
        .collect();
    assert_eq!(modules, ["Module 2", "Module 10"]);
    assert_eq!(plan.modules[0].lessons[0].file_name, "b.mp4");
    let mut reported = skipped(&plan);
    reported.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        reported,
        [
            ("Course/Empty".to_owned(), SkipReason::NoLessons),
            (
                "Course/Empty/readme.md".to_owned(),
                SkipReason::UnsupportedType
            ),
            (
                "Course/Module 2/deeper".to_owned(),
                SkipReason::NestedFolder
            ),
            (
                "Course/syllabus.txt".to_owned(),
                SkipReason::UnsupportedType
            ),
        ]
    );

    // A folder with lessons of its own stays one module; folders inside it
    // are left out.
    scratch.file("Course/intro.mp4", b"x");
    let plan = plan_drop(&[scratch.0.join("Course")], DropLimits::default());
    assert_eq!(plan.modules.len(), 1);
    assert_eq!(plan.modules[0].title, "Course");
    assert!(skipped(&plan).contains(&("Course/Module 2".to_owned(), SkipReason::NestedFolder)));

    // The module limit also bounds a course folder.
    let limited = plan_drop(
        &[scratch.0.join("Course")],
        DropLimits {
            max_folders: 0,
            ..DropLimits::default()
        },
    );
    assert_eq!(limited.modules, []);
}

#[test]
fn the_inbox_hands_out_opaque_tokens_that_expire() {
    let inbox = DropInbox::new(2, Duration::from_mins(1));
    let plans: Vec<_> = (0..3)
        .map(|index| {
            let scratch = Scratch::new(&format!("inbox-{index}"));
            let path = scratch.file("a.pdf", b"x");
            plan_drop(&[path], DropLimits::default())
        })
        .collect();
    let first = inbox.insert(plans[0].clone()).unwrap();
    let second = inbox.insert(plans[1].clone()).unwrap();
    assert!(first.starts_with("drop_") && !first.contains('/'));
    assert_eq!(inbox.plan(&first), Some(plans[0].clone()));
    let third = inbox.insert(plans[2].clone()).unwrap();
    assert_eq!(inbox.plan(&first), None);
    assert_eq!(inbox.plan(&second), Some(plans[1].clone()));
    inbox.discard(&second);
    assert_eq!(inbox.plan(&second), None);
    assert!(inbox.plan(&third).is_some());
    assert_eq!(inbox.plan("drop_unknown"), None);

    let expiring = DropInbox::new(4, Duration::ZERO);
    let token = expiring.insert(plans[0].clone()).unwrap();
    assert_eq!(expiring.plan(&token), None);
}

/// Real-course check. Run with
/// `ZERON_DROP_FIXTURES=<folder>:<folder> cargo test -p publisher-application --test drops -- --ignored`.
#[test]
#[ignore = "reads user-provided course folders"]
fn real_course_folders_become_modules() {
    let folders: Vec<PathBuf> = std::env::var("ZERON_DROP_FIXTURES")
        .expect("set ZERON_DROP_FIXTURES")
        .split(':')
        .map(PathBuf::from)
        .collect();
    let plan = plan_drop(&folders, DropLimits::default());
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let outcome = drafts.apply_drop(&draft_id, 0, &plan, None).unwrap();
    for module in &outcome.draft.draft.modules {
        let titles: Vec<_> = module
            .lessons
            .iter()
            .map(|lesson| lesson.title.as_str())
            .collect();
        println!("{} ({} lessons): {titles:?}", module.title, titles.len());
        for lesson in &module.lessons {
            assert!(
                lesson.resources[0].duration_seconds.is_some(),
                "{}",
                lesson.title
            );
        }
    }
    println!("skipped: {:?}", outcome.skipped);
    assert_eq!(outcome.modules_added, folders.len());
    assert!(
        outcome
            .draft
            .draft
            .validate()
            .iter()
            .all(|issue| !issue.field.contains("resources"))
    );
}
