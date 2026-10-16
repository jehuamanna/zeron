//! Immutable release activation and recovery.

use std::{fs, path::PathBuf};

use learning_application::{CancellationFlag, ImportLimits, import_local_course};
use publisher_application::{
    DistributionOptions, DraftServiceError, MetadataEdit, PublishJobs, PublishStage,
    PublisherDrafts, RELEASE_PACKAGE_DIRECTORY, RELEASE_TORRENT_FILE, ReleaseFiles, ReleaseStatus,
    StructureEdit,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-app-releases-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn staging(&self) -> PathBuf {
        self.0.join("staging")
    }

    fn releases(&self) -> PathBuf {
        self.0.join("releases")
    }

    fn entries(path: &PathBuf) -> Vec<String> {
        fs::read_dir(path)
            .map(|entries| {
                let mut names: Vec<_> = entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                names
            })
            .unwrap_or_default()
    }
}

#[cfg(unix)]
impl Scratch {
    fn make_tree_writable(path: &std::path::Path) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                Self::make_tree_writable(&path);
            } else if metadata.is_file() {
                set_writable(&path, true);
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    scratch: Scratch,
    drafts: PublisherDrafts,
    jobs: PublishJobs,
    draft_id: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = drafts.create().unwrap().draft.draft_id;
        drafts
            .update_metadata(&draft_id, 0, metadata("1.0.0"))
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
        let source = scratch.0.join("lecture.pdf");
        fs::write(&source, vec![4_u8; 40_000]).unwrap();
        drafts
            .attach_source(&draft_id, 3, &lesson_id, &source)
            .unwrap();
        let jobs = PublishJobs::new(&scratch.staging());
        Self {
            scratch,
            drafts,
            jobs,
            draft_id,
        }
    }

    fn prepare(&mut self) -> String {
        let draft = self.drafts.draft(&self.draft_id).unwrap().unwrap().draft;
        let job = self.jobs.begin(&draft).unwrap();
        let finished = self
            .jobs
            .run(
                &job.job_id,
                &draft,
                &DistributionOptions::default(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(finished.stage, PublishStage::ReadyToPublish);
        job.job_id
    }

    fn set_version(&mut self, version: &str) {
        let revision = self
            .drafts
            .draft(&self.draft_id)
            .unwrap()
            .unwrap()
            .draft
            .revision;
        self.drafts
            .update_metadata(&self.draft_id, revision, metadata(version))
            .unwrap();
    }
}

fn metadata(version: &str) -> MetadataEdit {
    MetadataEdit {
        version: version.to_owned(),
        title: "Released Course".to_owned(),
        publisher_name: "Zeron Tests".to_owned(),
        language: "en".to_owned(),
        ..MetadataEdit::default()
    }
}

#[test]
fn activation_moves_verified_artifacts_into_an_immutable_release() {
    let mut fixture = Fixture::new("activate");
    let job_id = fixture.prepare();
    let prepared = fixture.jobs.snapshot(&job_id).unwrap().prepared.unwrap();
    let (record, snapshot) = fixture
        .jobs
        .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();

    assert!(record.release.release_id.starts_with("release_"));
    assert_eq!(record.release.course_version, "1.0.0");
    assert_eq!(record.release.title, "Released Course");
    assert_eq!(record.release.info_hash, prepared.torrent.info_hash);
    assert_eq!(
        record.release.manifest_sha256,
        prepared.staging.manifest_sha256
    );
    assert_eq!(record.release.draft_revision, 4);
    assert_eq!(record.status, ReleaseStatus::Published);
    assert_eq!(snapshot.stage, PublishStage::Published);
    assert_eq!(
        snapshot.release_id.as_deref(),
        Some(record.release.release_id.as_str())
    );
    assert_eq!(Scratch::entries(&fixture.scratch.staging()).len(), 0);
    assert_eq!(
        Scratch::entries(&fixture.scratch.releases()),
        std::slice::from_ref(&record.release.release_id)
    );

    let release = fixture.scratch.releases().join(&record.release.release_id);
    let package = release.join(RELEASE_PACKAGE_DIRECTORY);
    let torrent = release.join(RELEASE_TORRENT_FILE);
    assert!(fs::metadata(&torrent).unwrap().permissions().readonly());
    assert!(
        fs::metadata(package.join("course.json"))
            .unwrap()
            .permissions()
            .readonly()
    );
    assert!(fs::write(package.join("course.json"), b"tampered").is_err());
    let imported = import_local_course(
        &package,
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();
    assert_eq!(imported.manifest.version, "1.0.0");
    assert_eq!(fixture.drafts.releases().unwrap(), vec![record]);

    assert!(matches!(
        fixture
            .jobs
            .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases()),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
    assert!(!fixture.jobs.cancel(&job_id));
}

#[test]
fn versions_are_never_reused_and_newer_releases_supersede_older_ones() {
    let mut fixture = Fixture::new("versions");
    let first = fixture.prepare();
    let (first_record, _) = fixture
        .jobs
        .activate(&first, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();

    let repeat = fixture.prepare();
    let attempt = fixture
        .jobs
        .snapshot(&repeat)
        .unwrap()
        .prepared
        .unwrap()
        .staging
        .attempt_id;
    assert!(matches!(
        fixture
            .jobs
            .activate(&repeat, &mut fixture.drafts, &fixture.scratch.releases()),
        Err(DraftServiceError::VersionAlreadyReleased { course_version }) if course_version == "1.0.0"
    ));
    assert!(fixture.scratch.staging().join(&attempt).is_dir());
    assert_eq!(Scratch::entries(&fixture.scratch.releases()).len(), 1);
    assert_eq!(
        fixture.jobs.snapshot(&repeat).unwrap().stage,
        PublishStage::ReadyToPublish
    );

    fixture.set_version("1.1.0");
    let second = fixture.prepare();
    let (second_record, _) = fixture
        .jobs
        .activate(&second, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let releases = fixture.drafts.releases().unwrap();
    let status = |id: &str| {
        releases
            .iter()
            .find(|record| record.release.release_id == id)
            .unwrap()
            .status
    };
    assert_eq!(
        status(&first_record.release.release_id),
        ReleaseStatus::Superseded
    );
    assert_eq!(
        status(&second_record.release.release_id),
        ReleaseStatus::Published
    );
    assert_ne!(
        first_record.release.info_hash,
        second_record.release.info_hash
    );
}

#[test]
fn artifacts_changed_after_verification_are_not_activated() {
    let mut fixture = Fixture::new("tamper");
    let job_id = fixture.prepare();
    let attempt = fixture
        .jobs
        .snapshot(&job_id)
        .unwrap()
        .prepared
        .unwrap()
        .staging
        .attempt_id;
    fs::write(
        fixture.scratch.staging().join(&attempt).join("extra.bin"),
        b"x",
    )
    .unwrap();
    assert!(matches!(
        fixture
            .jobs
            .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases()),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
    assert_eq!(fixture.drafts.releases().unwrap().len(), 0);
    assert_eq!(Scratch::entries(&fixture.scratch.releases()).len(), 0);
    assert!(fixture.scratch.staging().join(&attempt).is_dir());
}

#[test]
fn startup_recovery_completes_recorded_activations_and_drops_unrecorded_ones() {
    let mut fixture = Fixture::new("recover");
    let job_id = fixture.prepare();
    let (record, _) = fixture
        .jobs
        .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let releases = fixture.scratch.releases();
    let final_path = releases.join(&record.release.release_id);
    let interrupted = releases.join(format!("{}.partial", record.release.release_id));
    fs::rename(&final_path, &interrupted).unwrap();
    let orphan = releases.join("release_0123456789abcdef0123456789abcdef.partial");
    fs::create_dir_all(orphan.join(RELEASE_PACKAGE_DIRECTORY)).unwrap();
    fs::write(
        orphan.join(RELEASE_PACKAGE_DIRECTORY).join("course.json"),
        b"{}",
    )
    .unwrap();
    let unrelated = releases.join("notes.partial");
    fs::create_dir_all(&unrelated).unwrap();

    assert_eq!(fixture.drafts.recover_releases(&releases).unwrap(), 2);
    assert!(
        final_path
            .join(RELEASE_PACKAGE_DIRECTORY)
            .join("course.json")
            .is_file()
    );
    assert!(!interrupted.exists());
    assert!(!orphan.exists());
    assert!(unrelated.is_dir());

    let leftover = fixture.prepare();
    let attempt = fixture
        .jobs
        .snapshot(&leftover)
        .unwrap()
        .prepared
        .unwrap()
        .staging
        .attempt_id;
    assert!(fixture.scratch.staging().join(&attempt).is_dir());
    let restarted = PublishJobs::new(&fixture.scratch.staging());
    assert_eq!(restarted.recover().unwrap(), 2);
    assert_eq!(Scratch::entries(&fixture.scratch.staging()).len(), 0);
}

#[test]
fn recorded_releases_can_be_fully_reverified() {
    let mut fixture = Fixture::new("reverify");
    let job_id = fixture.prepare();
    let (record, _) = fixture
        .jobs
        .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let release_id = record.release.release_id.clone();
    let cancellation = publisher_application::PackagingCancellation::default();
    let mut hashed = 0;
    let verified = fixture
        .drafts
        .verify_release(
            &fixture.scratch.releases(),
            &release_id,
            &cancellation,
            &mut |progress| {
                hashed = progress.hashed_bytes;
            },
        )
        .unwrap();
    assert_eq!(verified, record);
    assert_eq!(hashed, record.release.total_bytes);

    for id in [
        "release_missing",
        "../releases",
        "release_0123456789abcdef0123456789abcdef",
    ] {
        assert!(matches!(
            fixture.drafts.verify_release(
                &fixture.scratch.releases(),
                id,
                &cancellation,
                &mut |_| {}
            ),
            Err(DraftServiceError::InvalidAttempt(_))
        ));
    }

    let package = fixture
        .scratch
        .releases()
        .join(&release_id)
        .join(RELEASE_PACKAGE_DIRECTORY)
        .join("resources");
    let resource_directory = fs::read_dir(&package)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let resource = fs::read_dir(&resource_directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut permissions = fs::metadata(&resource).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "simulating tampering"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&resource, permissions).unwrap();
    let mut bytes = fs::read(&resource).unwrap();
    bytes[0] ^= 0xff;
    fs::write(&resource, bytes).unwrap();
    assert!(matches!(
        fixture.drafts.verify_release(
            &fixture.scratch.releases(),
            &release_id,
            &cancellation,
            &mut |_| {}
        ),
        Err(DraftServiceError::Torrent(
            publisher_application::TorrentError::Verification(_)
        ))
    ));
}

#[test]
fn recorded_release_torrents_are_read_back_only_while_they_match_the_record() {
    let mut fixture = Fixture::new("export");
    let job_id = fixture.prepare();
    let (record, _) = fixture
        .jobs
        .activate(&job_id, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let release_id = record.release.release_id.clone();
    let torrent_path = fixture
        .scratch
        .releases()
        .join(&release_id)
        .join(RELEASE_TORRENT_FILE);

    let exported = fixture
        .drafts
        .release_torrent(&fixture.scratch.releases(), &release_id)
        .unwrap();
    assert_eq!(exported.record, record);
    assert_eq!(exported.bytes, fs::read(&torrent_path).unwrap());
    assert_eq!(exported.files.len() as u64, record.release.file_count);
    assert_eq!(
        exported.files.iter().map(|file| file.length).sum::<u64>(),
        record.release.total_bytes
    );
    assert!(exported.files.iter().any(|file| file.path == "course.json"));
    assert!(
        exported
            .files
            .iter()
            .all(|file| !file.path.starts_with('/') && !file.path.contains(".."))
    );

    for id in [
        "release_missing",
        "../releases",
        "release_0123456789abcdef0123456789abcdef",
    ] {
        assert!(matches!(
            fixture
                .drafts
                .release_torrent(&fixture.scratch.releases(), id),
            Err(DraftServiceError::InvalidAttempt(_))
        ));
    }

    let mut permissions = fs::metadata(&torrent_path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "simulating tampering"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&torrent_path, permissions).unwrap();
    let mut tampered = exported.bytes.clone();
    let position = tampered
        .windows(b"course.json".len())
        .position(|window| window == b"course.json")
        .unwrap();
    tampered[position] = b'C';
    fs::write(&torrent_path, tampered).unwrap();
    assert!(matches!(
        fixture
            .drafts
            .release_torrent(&fixture.scratch.releases(), &release_id),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}

#[cfg(unix)]
fn set_writable(path: &std::path::Path, writable: bool) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(!writable);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn only_archived_releases_lose_their_files_and_only_with_the_version_typed() {
    let mut fixture = Fixture::new("delete-files");
    let first = fixture.prepare();
    let (older, _) = fixture
        .jobs
        .activate(&first, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    fixture.set_version("1.1.0");
    let second = fixture.prepare();
    let (newer, _) = fixture
        .jobs
        .activate(&second, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let root = fixture.scratch.releases();
    let (older_id, newer_id) = (older.release.release_id, newer.release.release_id);

    for id in [&older_id, &newer_id] {
        assert!(matches!(
            fixture.drafts.delete_release_files(&root, id, "1.0.0"),
            Err(DraftServiceError::ReleaseNotArchived)
        ));
    }
    for id in [
        "../releases",
        "release_missing",
        "release_0123456789abcdef0123456789abcdef",
    ] {
        assert!(matches!(
            fixture.drafts.delete_release_files(&root, id, "1.0.0"),
            Err(DraftServiceError::InvalidAttempt(_))
        ));
    }
    fixture.drafts.archive_release(&older_id).unwrap();
    for typed in ["", "1.1.0", "1.0", "Released Course"] {
        assert!(matches!(
            fixture.drafts.delete_release_files(&root, &older_id, typed),
            Err(DraftServiceError::InvalidInput { ref field, .. }) if field == "confirmation"
        ));
    }
    assert!(root.join(&older_id).join(RELEASE_TORRENT_FILE).is_file());

    let deleted = fixture
        .drafts
        .delete_release_files(&root, &older_id, " 1.0.0 ")
        .unwrap();
    assert_eq!(deleted.status, ReleaseStatus::Archived);
    assert_eq!(deleted.files, ReleaseFiles::Deleted);
    assert_eq!(Scratch::entries(&root), std::slice::from_ref(&newer_id));
    assert_eq!(
        fixture
            .drafts
            .delete_release_files(&root, &older_id, "")
            .unwrap(),
        deleted
    );
    assert!(fixture.drafts.release_torrent(&root, &older_id).is_err());
    assert!(matches!(
        fixture.drafts.seed_request(&root, &older_id),
        Err(DraftServiceError::ReleaseArchived)
    ));
    fixture.set_version("1.0.0");
    let reused = fixture.prepare();
    assert!(matches!(
        fixture.jobs.activate(&reused, &mut fixture.drafts, &root),
        Err(DraftServiceError::VersionAlreadyReleased { .. })
    ));
    assert_eq!(
        fixture.drafts.release(&newer_id).unwrap().unwrap().files,
        ReleaseFiles::Present
    );
}

#[cfg(unix)]
#[test]
fn deleting_release_files_never_follows_symbolic_links() {
    let mut fixture = Fixture::new("delete-links");
    let outside = fixture.scratch.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"keep").unwrap();
    let root = fixture.scratch.releases();

    let mut ids = Vec::new();
    for version in ["1.0.0", "1.1.0"] {
        fixture.set_version(version);
        let job = fixture.prepare();
        let (record, _) = fixture
            .jobs
            .activate(&job, &mut fixture.drafts, &root)
            .unwrap();
        fixture
            .drafts
            .archive_release(&record.release.release_id)
            .unwrap();
        ids.push(record.release.release_id);
    }
    let package = root.join(&ids[0]).join(RELEASE_PACKAGE_DIRECTORY);
    std::os::unix::fs::symlink(&outside, package.join("escape")).unwrap();
    std::os::unix::fs::symlink(outside.join("keep.txt"), package.join("escape.txt")).unwrap();
    fixture
        .drafts
        .delete_release_files(&root, &ids[0], "1.0.0")
        .unwrap();

    Scratch::make_tree_writable(&root.join(&ids[1]));
    fs::remove_dir_all(root.join(&ids[1])).unwrap();
    std::os::unix::fs::symlink(&outside, root.join(&ids[1])).unwrap();
    fixture
        .drafts
        .delete_release_files(&root, &ids[1], "1.1.0")
        .unwrap();

    assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"keep");
    assert_eq!(Scratch::entries(&root).len(), 0);
}

#[cfg(unix)]
#[test]
fn startup_finishes_interrupted_and_half_done_file_deletions() {
    let mut fixture = Fixture::new("delete-recover");
    let job = fixture.prepare();
    let root = fixture.scratch.releases();
    let (record, _) = fixture
        .jobs
        .activate(&job, &mut fixture.drafts, &root)
        .unwrap();
    let release_id = record.release.release_id;
    fixture.drafts.archive_release(&release_id).unwrap();

    set_writable(&root, false);
    let interrupted = fixture
        .drafts
        .delete_release_files(&root, &release_id, "1.0.0");
    set_writable(&root, true);
    assert!(matches!(interrupted, Err(DraftServiceError::Torrent(_))));
    assert_eq!(
        fixture.drafts.release(&release_id).unwrap().unwrap().files,
        ReleaseFiles::Deleting
    );
    assert!(root.join(&release_id).is_dir());

    let aside = root.join(format!("{release_id}.deleting"));
    fs::rename(root.join(&release_id), &aside).unwrap();
    fs::remove_file(aside.join(RELEASE_TORRENT_FILE)).unwrap();
    assert_eq!(fixture.drafts.recover_releases(&root).unwrap(), 0);
    assert!(aside.is_dir());

    assert_eq!(fixture.drafts.recover_release_deletions(&root).unwrap(), 1);
    assert_eq!(Scratch::entries(&root).len(), 0);
    assert_eq!(
        fixture.drafts.release(&release_id).unwrap().unwrap().files,
        ReleaseFiles::Deleted
    );
    assert_eq!(fixture.drafts.recover_release_deletions(&root).unwrap(), 0);
}

#[test]
fn an_older_release_can_be_made_current_and_archived_ones_cannot() {
    let mut fixture = Fixture::new("make-current");
    let first = fixture.prepare();
    let (first_record, _) = fixture
        .jobs
        .activate(&first, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    fixture.set_version("1.1.0");
    let second = fixture.prepare();
    let (second_record, _) = fixture
        .jobs
        .activate(&second, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let first_id = first_record.release.release_id;
    let second_id = second_record.release.release_id;

    let current = fixture.drafts.make_release_current(&first_id).unwrap();
    assert_eq!(current.status, ReleaseStatus::Published);
    let status = |drafts: &PublisherDrafts, id: &str| {
        drafts
            .releases()
            .unwrap()
            .into_iter()
            .find(|record| record.release.release_id == id)
            .unwrap()
            .status
    };
    assert_eq!(
        status(&fixture.drafts, &second_id),
        ReleaseStatus::Superseded
    );

    fixture.drafts.archive_release(&second_id).unwrap();
    assert!(matches!(
        fixture.drafts.make_release_current(&second_id),
        Err(DraftServiceError::ReleaseArchived)
    ));
    assert!(matches!(
        fixture.drafts.make_release_current("release_unknown"),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
    assert_eq!(status(&fixture.drafts, &first_id), ReleaseStatus::Published);
}

#[test]
fn a_new_draft_from_a_release_keeps_its_ids_and_publishes_the_next_version() {
    let mut fixture = Fixture::new("draft-from-release");
    let first = fixture.prepare();
    let (record, _) = fixture
        .jobs
        .activate(&first, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let original = fixture
        .drafts
        .draft(&fixture.draft_id)
        .unwrap()
        .unwrap()
        .draft;
    let release_id = record.release.release_id.clone();

    let stored = fixture
        .drafts
        .draft_from_release(&fixture.scratch.releases(), &release_id)
        .unwrap();
    let draft = stored.draft;
    assert_ne!(draft.draft_id, original.draft_id);
    assert_eq!(draft.revision, 0);
    assert_eq!(draft.metadata.course_id, original.metadata.course_id);
    assert_eq!(draft.metadata.version, "1.0.1");
    assert_eq!(draft.metadata.title, "Released Course");
    assert_eq!(draft.metadata.publisher_name, "Zeron Tests");
    assert_eq!(draft.modules.len(), 1);
    assert_eq!(draft.modules[0].id, original.modules[0].id);
    let lesson = &draft.modules[0].lessons[0];
    assert_eq!(lesson.id, original.modules[0].lessons[0].id);
    let resource = &lesson.resources[0];
    let original_resource = &original.modules[0].lessons[0].resources[0];
    assert_eq!(resource.id, original_resource.id);
    assert_eq!(resource.package_path, original_resource.package_path);
    let package = fs::canonicalize(
        fixture
            .scratch
            .releases()
            .join(&release_id)
            .join(RELEASE_PACKAGE_DIRECTORY),
    )
    .unwrap();
    assert!(resource.source_path.starts_with(&package));
    assert!(
        fixture
            .drafts
            .list()
            .unwrap()
            .iter()
            .any(|summary| summary.draft_id == draft.draft_id)
    );

    let job = fixture.jobs.begin(&draft).unwrap();
    let finished = fixture
        .jobs
        .run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(finished.stage, PublishStage::ReadyToPublish);
    let (next, _) = fixture
        .jobs
        .activate(
            &job.job_id,
            &mut fixture.drafts,
            &fixture.scratch.releases(),
        )
        .unwrap();
    assert_eq!(next.release.course_version, "1.0.1");
    assert_eq!(next.release.course_id, record.release.course_id);

    assert!(matches!(
        fixture
            .drafts
            .draft_from_release(&fixture.scratch.releases(), "../release_x"),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}

#[test]
fn a_release_without_its_files_cannot_start_a_draft() {
    let mut fixture = Fixture::new("draft-from-deleted");
    let first = fixture.prepare();
    let (record, _) = fixture
        .jobs
        .activate(&first, &mut fixture.drafts, &fixture.scratch.releases())
        .unwrap();
    let release_id = record.release.release_id;
    fixture.drafts.archive_release(&release_id).unwrap();
    fixture
        .drafts
        .delete_release_files(&fixture.scratch.releases(), &release_id, "1.0.0")
        .unwrap();
    assert!(matches!(
        fixture
            .drafts
            .draft_from_release(&fixture.scratch.releases(), &release_id),
        Err(DraftServiceError::InvalidAttempt(
            "the release files were deleted"
        ))
    ));
}
