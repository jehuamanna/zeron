//! Draft application service tests.

use publisher_application::{DraftServiceError, MetadataEdit, PublisherDrafts};

fn edit(title: &str) -> MetadataEdit {
    MetadataEdit {
        version: "1.0.0".to_owned(),
        title: title.to_owned(),
        description: Some("Line one\nLine two".to_owned()),
        publisher_name: "Zeron Tests".to_owned(),
        language: "en".to_owned(),
        category: Some("  ".to_owned()),
        tags: vec![
            " rust ".to_owned(),
            String::new(),
            "rust".to_owned(),
            "systems".to_owned(),
        ],
        prerequisites: vec![],
        license_name: None,
    }
}

#[test]
fn create_assigns_opaque_ids_and_defaults() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let first = drafts.create().unwrap();
    let second = drafts.create().unwrap();
    assert_eq!(first.draft.revision, 0);
    assert_eq!(first.draft.metadata.version, "0.1.0");
    assert_eq!(first.draft.metadata.title, "Untitled course");
    assert_eq!(first.draft.metadata.publisher_name, "Local publisher");
    assert_eq!(first.draft.metadata.language, "en");
    assert_eq!(first.draft.modules.len(), 0);
    for (id, prefix) in [
        (&first.draft.draft_id, "draft_"),
        (&first.draft.metadata.course_id, "course_"),
    ] {
        let hex = id.strip_prefix(prefix).unwrap();
        assert_eq!(hex.len(), 32);
        assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_ne!(first.draft.draft_id, second.draft.draft_id);
    assert_eq!(drafts.list().unwrap().len(), 2);
}

#[test]
fn metadata_autosave_advances_revision_and_keeps_course_identity() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let created = drafts.create().unwrap();
    let draft_id = created.draft.draft_id.clone();
    let saved = drafts
        .update_metadata(&draft_id, 0, edit("Systems Programming"))
        .unwrap();
    assert_eq!(saved.draft.revision, 1);
    assert_eq!(
        saved.draft.metadata.course_id,
        created.draft.metadata.course_id
    );
    assert_eq!(saved.draft.metadata.category, None);
    assert_eq!(saved.draft.metadata.tags, ["rust", "systems"]);
    let reloaded = drafts.draft(&draft_id).unwrap().unwrap();
    assert_eq!(reloaded.draft, saved.draft);
    assert_eq!(drafts.list().unwrap()[0].title, "Systems Programming");
}

#[test]
fn stale_autosave_reports_conflict_and_preserves_first_save() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(&draft_id, 0, edit("First editor"))
        .unwrap();
    let error = drafts
        .update_metadata(&draft_id, 0, edit("Stale editor"))
        .unwrap_err();
    assert!(matches!(
        error,
        DraftServiceError::RevisionConflict {
            expected: 0,
            actual: 1
        }
    ));
    let durable = drafts.draft(&draft_id).unwrap().unwrap();
    assert_eq!(durable.draft.metadata.title, "First editor");
    assert_eq!(durable.draft.revision, 1);
}

#[test]
fn rejects_unbounded_or_control_input_without_writing() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let cases = [
        (edit(&"x".repeat(201)), "metadata.title"),
        (edit("Title\u{0}"), "metadata.title"),
        (
            MetadataEdit {
                description: Some("bell\u{7}".to_owned()),
                ..edit("Title")
            },
            "metadata.description",
        ),
        (
            MetadataEdit {
                tags: vec!["tag".to_owned(); 33],
                ..edit("Title")
            },
            "metadata.tags",
        ),
        (
            MetadataEdit {
                tags: vec!["two\nlines".to_owned()],
                ..edit("Title")
            },
            "metadata.tags[0]",
        ),
    ];
    for (input, expected_field) in cases {
        match drafts.update_metadata(&draft_id, 0, input) {
            Err(DraftServiceError::InvalidInput { field, .. }) => assert_eq!(field, expected_field),
            other => panic!("expected invalid input for {expected_field}, got {other:?}"),
        }
    }
    assert_eq!(drafts.draft(&draft_id).unwrap().unwrap().draft.revision, 0);
}

#[test]
fn missing_draft_is_reported_for_edits_and_validation() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert!(drafts.draft("draft_missing").unwrap().is_none());
    assert!(matches!(
        drafts.update_metadata("draft_missing", 0, edit("Title")),
        Err(DraftServiceError::NotFound { .. })
    ));
    assert!(matches!(
        drafts.validate("draft_missing"),
        Err(DraftServiceError::NotFound { .. })
    ));
}

#[test]
fn validation_reports_blocking_issues_for_saved_draft() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                publisher_name: " ".to_owned(),
                ..edit("Title")
            },
        )
        .unwrap();
    let issues = drafts.validate(&draft_id).unwrap();
    assert!(issues.iter().any(|issue| issue.code == "empty_course"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.field == "metadata.publisherName")
    );
}

#[test]
fn file_backed_drafts_survive_reopen() {
    let database = std::env::temp_dir().join(format!(
        "zeron-publisher-application-{}.sqlite3",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&database);
    let draft_id = {
        let mut drafts = PublisherDrafts::open(&database).unwrap();
        let draft_id = drafts.create().unwrap().draft.draft_id;
        drafts
            .update_metadata(&draft_id, 0, edit("Durable"))
            .unwrap();
        draft_id
    };
    let drafts = PublisherDrafts::open(&database).unwrap();
    let recovered = drafts.draft(&draft_id).unwrap().unwrap();
    assert_eq!(recovered.draft.metadata.title, "Durable");
    assert_eq!(recovered.draft.revision, 1);
    std::fs::remove_file(database).unwrap();
}

#[test]
fn deleting_a_draft_checks_its_revision_and_leaves_others() {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let kept = drafts.create().unwrap().draft.draft_id;
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts.update_metadata(&draft_id, 0, edit("Saved")).unwrap();

    assert!(matches!(
        drafts.delete_draft(&draft_id, 0),
        Err(DraftServiceError::RevisionConflict {
            expected: 0,
            actual: 1
        })
    ));
    drafts.delete_draft(&draft_id, 1).unwrap();
    assert!(drafts.draft(&draft_id).unwrap().is_none());
    assert!(matches!(
        drafts.delete_draft(&draft_id, 1),
        Err(DraftServiceError::NotFound { .. })
    ));
    let remaining: Vec<_> = drafts
        .list()
        .unwrap()
        .into_iter()
        .map(|summary| summary.draft_id)
        .collect();
    assert_eq!(remaining, [kept]);
}
