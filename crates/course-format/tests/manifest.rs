//! Contract and validation tests for schema version one.

use std::{fs, path::PathBuf};

use course_format::{ManifestError, ResourceKind, parse_and_validate};
use serde_json::{Value, json};

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn valid_fixture_round_trips() {
    let manifest = parse_and_validate(&fixture("valid-course.json")).unwrap();
    assert_eq!(manifest.course_id, "course_distributed_systems");
    assert_eq!(
        manifest.modules[0].lessons[0].resources[0].kind,
        ResourceKind::Video
    );

    let serialized = serde_json::to_vec(&manifest).unwrap();
    assert_eq!(manifest, parse_and_validate(&serialized).unwrap());
}

#[test]
fn rejects_unknown_fields_during_parsing() {
    let mut value: Value = serde_json::from_slice(&fixture("valid-course.json")).unwrap();
    value["surprise"] = json!(true);
    assert!(matches!(
        parse_and_validate(&serde_json::to_vec(&value).unwrap()),
        Err(ManifestError::Json(_))
    ));
}

#[test]
fn reports_multiple_semantic_failures() {
    let mut value: Value = serde_json::from_slice(&fixture("valid-course.json")).unwrap();
    value["version"] = json!("not-semver");
    value["modules"][0]["lessons"][0]["resources"][0]["path"] = json!("../escape.mp4");
    value["modules"][0]["lessons"][0]["resources"][1]["id"] = json!("res_video");
    value["modules"][0]["lessons"][0]["resources"][0]["relations"][0]["resourceId"] =
        json!("missing");

    let Err(ManifestError::Validation(issues)) =
        parse_and_validate(&serde_json::to_vec(&value).unwrap())
    else {
        panic!("expected semantic validation failure");
    };
    assert!(issues.len() >= 4, "{issues:#?}");
    assert!(issues.iter().any(|issue| issue.path == "version"));
    assert!(
        issues
            .iter()
            .any(|issue| issue.path.rsplit('.').next() == Some("path"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("duplicates ID"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("unknown resource"))
    );
}

#[test]
fn rejects_cross_platform_path_collisions() {
    let mut value: Value = serde_json::from_slice(&fixture("valid-course.json")).unwrap();
    value["modules"][0]["lessons"][0]["resources"][1]["path"] =
        json!("modules/INTRODUCTION/WELCOME.MP4");
    let Err(ManifestError::Validation(issues)) =
        parse_and_validate(&serde_json::to_vec(&value).unwrap())
    else {
        panic!("expected path collision");
    };
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("case collision"))
    );
}

#[test]
fn rejects_unsafe_and_non_portable_package_paths() {
    let unsafe_paths = [
        "/absolute/video.mp4",
        "../escape.mp4",
        "modules\\windows.mp4",
        "C:/drive.mp4",
        "assets/CON.png",
        "assets/trailing-dot.",
        "assets/trailing-space ",
        "course.json",
        "modules/cafe\u{301}.mp4",
    ];

    for unsafe_path in unsafe_paths {
        let mut value: Value = serde_json::from_slice(&fixture("valid-course.json")).unwrap();
        value["modules"][0]["lessons"][0]["resources"][0]["path"] = json!(unsafe_path);
        let result = parse_and_validate(&serde_json::to_vec(&value).unwrap());
        assert!(
            matches!(result, Err(ManifestError::Validation(_))),
            "accepted unsafe path {unsafe_path:?}"
        );
    }
}

#[test]
fn generated_schema_matches_checked_in_contract() {
    let expected: Value = serde_json::from_slice(
        &fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("schemas/course-manifest-v1.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let actual = serde_json::to_value(course_format::manifest_schema()).unwrap();
    assert_eq!(
        expected, actual,
        "run `cargo run -p course-format --bin export-course-schema`"
    );
}
