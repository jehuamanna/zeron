//! End-to-end publisher CLI tests against a real data directory.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use learning_application::{CancellationFlag, ImportLimits, import_local_course};
use publisher_application::{MetadataEdit, PublisherDrafts, StructureEdit};

struct DataDirectory(PathBuf);

impl DataDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("zeron-cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn cli(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_publisher-cli"))
            .arg(&self.0)
            .args(arguments)
            .output()
            .unwrap()
    }

    fn entries(&self, child: &str) -> Vec<String> {
        fs::read_dir(self.0.join(child))
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn publishable_draft(data: &Path) -> String {
    let mut drafts = PublisherDrafts::open(&data.join("publisher.sqlite3")).unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: "3.0.0".to_owned(),
                title: "CLI Course".to_owned(),
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
    let source = data.join("source.pdf");
    fs::write(&source, vec![9_u8; 70_000]).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap();
    draft_id
}

#[test]
fn the_cli_prepares_publishes_and_verifies_releases() {
    let data = DataDirectory::new("publish");
    let draft_id = publishable_draft(&data.0);

    let drafts = data.cli(&["drafts"]);
    assert!(drafts.status.success(), "{}", stderr(&drafts));
    assert!(stdout(&drafts).contains(&format!("{draft_id}\t3.0.0\tr4\tCLI Course")));

    let dry_run = data.cli(&["prepare", &draft_id]);
    assert!(dry_run.status.success(), "{}", stderr(&dry_run));
    assert!(stdout(&dry_run).contains("prepared version 3.0.0: 2 files"));
    assert!(stdout(&dry_run).contains("dry run complete; nothing was published"));
    assert!(stderr(&dry_run).contains("verifying torrent independently"));
    let dry_hash = stdout(&dry_run)
        .lines()
        .find_map(|line| line.strip_prefix("info hash "))
        .unwrap()
        .to_owned();
    assert!(data.entries("cli-staging").is_empty());
    assert!(data.entries("releases").is_empty());

    let unconfirmed = data.cli(&["publish", &draft_id]);
    assert!(!unconfirmed.status.success());
    assert!(stderr(&unconfirmed).contains("--confirm"));
    assert!(data.entries("releases").is_empty());

    let published = data.cli(&["publish", &draft_id, "--confirm"]);
    assert!(published.status.success(), "{}", stderr(&published));
    let line = stdout(&published)
        .lines()
        .find(|line| line.starts_with("published "))
        .unwrap()
        .to_owned();
    let release_id = line.split_whitespace().nth(1).unwrap().to_owned();
    assert!(line.contains(&dry_hash), "deterministic info hash expected");
    assert!(data.entries("cli-staging").is_empty());

    let releases = data.cli(&["releases"]);
    assert!(stdout(&releases).contains(&format!(
        "{release_id}\t3.0.0\tpublished\t{dry_hash}\tCLI Course"
    )));

    let verified = data.cli(&["verify-release", &release_id]);
    assert!(verified.status.success(), "{}", stderr(&verified));
    assert!(stdout(&verified).contains(&format!("verified {release_id} version 3.0.0")));
    let package = data.0.join("releases").join(&release_id).join("package");
    import_local_course(
        &package,
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();

    let again = data.cli(&["publish", &draft_id, "--confirm"]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("already released"));
    assert!(
        !stderr(&again).contains("copying files"),
        "rejected before staging"
    );
    assert!(data.entries("cli-staging").is_empty());
    assert_eq!(data.entries("releases").len(), 1);
}

#[test]
fn the_cli_rejects_bad_input_and_cleans_only_its_own_staging() {
    let data = DataDirectory::new("errors");
    let missing_draft = data.cli(&["prepare", "draft_missing"]);
    assert!(!missing_draft.status.success());
    assert!(stderr(&missing_draft).contains("draft was not found"));
    let unknown_release = data.cli(&["verify-release", "release_missing"]);
    assert!(!unknown_release.status.success());
    assert!(stderr(&unknown_release).contains("unknown release"));
    let usage = data.cli(&["frobnicate"]);
    assert!(!usage.status.success());
    assert!(stderr(&usage).contains("usage: publisher-cli"));
    let no_directory = Command::new(env!("CARGO_BIN_EXE_publisher-cli"))
        .arg(data.0.join("missing"))
        .arg("drafts")
        .output()
        .unwrap();
    assert!(!no_directory.status.success());

    let admin_attempt = data
        .0
        .join("staging/stage_0123456789abcdef0123456789abcdef");
    fs::create_dir_all(&admin_attempt).unwrap();
    fs::create_dir_all(
        data.0
            .join("cli-staging/stage_0123456789abcdef0123456789abcdef.partial"),
    )
    .unwrap();
    let cleaned = data.cli(&["clean"]);
    assert!(cleaned.status.success(), "{}", stderr(&cleaned));
    assert!(stdout(&cleaned).contains("removed 1 CLI staging entries"));
    assert!(admin_attempt.is_dir());
}
