//! Publishing signed releases.

use std::{fs, path::PathBuf};

use course_signing::{RELEASE_SIGNATURE_FILE, parse_release_signature, verify_release};
use learning_application::{CancellationFlag, ImportLimits, import_local_course};
use publisher_application::{
    DistributionOptions, DraftServiceError, MetadataEdit, PublishJobs, PublishStage,
    PublisherDrafts, RELEASE_PACKAGE_DIRECTORY, ReleaseSigner, StructureEdit,
};
use publisher_identity::{MemoryStore, create, rotate_key, signing_key};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        make_writable(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_writable(path: &std::path::Path) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            make_writable(&path);
        } else {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
}

fn draft(
    scratch: &Scratch,
    drafts: &mut PublisherDrafts,
    version: &str,
) -> course_publisher::DraftCourse {
    let draft_id = drafts.create().unwrap().draft.draft_id;
    drafts
        .update_metadata(
            &draft_id,
            0,
            MetadataEdit {
                version: version.to_owned(),
                title: "Signed Course".to_owned(),
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
                title: "M".to_owned(),
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
                title: "L".to_owned(),
            },
        )
        .unwrap()
        .draft
        .modules[0]
        .lessons[0]
        .id
        .clone();
    let source = scratch.0.join(format!("lecture-{version}.pdf"));
    fs::write(&source, vec![5_u8; 20_000]).unwrap();
    drafts
        .attach_source(&draft_id, 3, &lesson_id, &source)
        .unwrap()
        .draft
}

#[test]
fn signed_releases_name_their_publisher_and_verify_after_a_rotation() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("zeron-signing-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).unwrap();
    let releases = scratch.0.join("releases");
    let store = MemoryStore::default();
    let identity = create(&store, 1).unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    assert!(drafts.identity().unwrap().is_none());
    drafts.set_identity(&identity).unwrap();
    assert_eq!(drafts.identity().unwrap(), Some(identity.clone()));
    assert!(drafts.sign_releases().unwrap());

    let jobs = PublishJobs::new(&scratch.0.join("staging"));
    let course = draft(&scratch, &mut drafts, "1.0.0");
    let signer = ReleaseSigner::new(
        identity.clone(),
        signing_key(&store, &identity).unwrap(),
        10,
    )
    .unwrap();
    let job = jobs.begin(&course).unwrap();
    let ready = jobs
        .run_signed(
            &job.job_id,
            &course,
            &DistributionOptions::default(),
            Some(&signer),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(ready.stage, PublishStage::ReadyToPublish);
    assert!(ready.prepared.as_ref().unwrap().staging.signed);
    assert_eq!(ready.prepared.as_ref().unwrap().torrent.file_count, 3);
    let (record, _) = jobs.activate(&job.job_id, &mut drafts, &releases).unwrap();

    let package = releases
        .join(&record.release.release_id)
        .join(RELEASE_PACKAGE_DIRECTORY);
    let manifest =
        course_format::parse_and_validate(&fs::read(package.join("course.json")).unwrap()).unwrap();
    assert_eq!(
        manifest.publisher.publisher_id.as_deref(),
        Some(identity.root.as_str())
    );
    let signature =
        parse_release_signature(&fs::read(package.join(RELEASE_SIGNATURE_FILE)).unwrap()).unwrap();
    let verified = verify_release(
        &signature,
        &manifest.course_id,
        "1.0.0",
        &record.release.manifest_sha256,
        &identity.root,
        &[],
    )
    .unwrap();
    assert_eq!(verified.signed_at, 10);
    assert_eq!(
        drafts
            .release_signature(&releases, &record.release.release_id)
            .unwrap()
            .unwrap()
            .root,
        identity.root_key().unwrap()
    );
    let files = drafts
        .release_torrent(&releases, &record.release.release_id)
        .unwrap()
        .files;
    assert!(files.iter().any(|file| file.path == RELEASE_SIGNATURE_FILE));
    let imported = import_local_course(
        &package,
        ImportLimits::default(),
        &CancellationFlag::default(),
    )
    .unwrap();
    assert_eq!(imported.manifest.version, "1.0.0");

    let rotated = rotate_key(&store, &identity, 20).unwrap();
    drafts.set_identity(&rotated).unwrap();
    let next = draft(&scratch, &mut drafts, "1.1.0");
    let signer =
        ReleaseSigner::new(rotated.clone(), signing_key(&store, &rotated).unwrap(), 30).unwrap();
    let job = jobs.begin(&next).unwrap();
    jobs.run_signed(
        &job.job_id,
        &next,
        &DistributionOptions::default(),
        Some(&signer),
        &mut |_| {},
    )
    .unwrap();
    let (second, _) = jobs.activate(&job.job_id, &mut drafts, &releases).unwrap();
    let verified = drafts
        .release_signature(&releases, &second.release.release_id)
        .unwrap()
        .unwrap();
    assert_eq!(verified.root, identity.root_key().unwrap());
    assert_eq!(verified.signing_key, rotated.current_key().unwrap());

    assert!(matches!(
        ReleaseSigner::new(identity.clone(), signing_key(&store, &rotated).unwrap(), 1),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}

#[test]
fn a_changed_signature_file_stops_activation() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("zeron-signing-tamper-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).unwrap();
    let store = MemoryStore::default();
    let identity = create(&store, 1).unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let jobs = PublishJobs::new(&scratch.0.join("staging"));
    let course = draft(&scratch, &mut drafts, "1.0.0");
    let signer = ReleaseSigner::new(
        identity.clone(),
        signing_key(&store, &identity).unwrap(),
        10,
    )
    .unwrap();
    let job = jobs.begin(&course).unwrap();
    let ready = jobs
        .run_signed(
            &job.job_id,
            &course,
            &DistributionOptions::default(),
            Some(&signer),
            &mut |_| {},
        )
        .unwrap();
    let attempt = scratch
        .0
        .join("staging")
        .join(&ready.prepared.unwrap().staging.attempt_id);
    let other = create(&MemoryStore::default(), 1).unwrap();
    let mut text = fs::read_to_string(attempt.join(RELEASE_SIGNATURE_FILE)).unwrap();
    text = text.replace(&identity.current, &other.current);
    fs::write(attempt.join(RELEASE_SIGNATURE_FILE), text).unwrap();
    assert!(matches!(
        jobs.activate(&job.job_id, &mut drafts, &scratch.0.join("releases")),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}
