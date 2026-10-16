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

fn publish_signed(
    scratch: &Scratch,
    drafts: &mut PublisherDrafts,
    jobs: &PublishJobs,
    (identity, store): (&publisher_identity::Identity, &MemoryStore),
    version: &str,
) -> publisher_application::ReleaseRecord {
    let course = draft(scratch, drafts, version);
    let signer =
        ReleaseSigner::new(identity.clone(), signing_key(store, identity).unwrap(), 10).unwrap();
    let job = jobs.begin(&course).unwrap();
    jobs.run_signed(
        &job.job_id,
        &course,
        &DistributionOptions::default(),
        Some(&signer),
        &mut |_| {},
    )
    .unwrap();
    jobs.activate(&job.job_id, drafts, &scratch.0.join("releases"))
        .unwrap()
        .0
}

#[test]
fn the_catalog_lists_confirmed_releases_and_carries_revocations() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("zeron-signing-catalog-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).unwrap();
    let releases = scratch.0.join("releases");
    let store = MemoryStore::default();
    let first_identity = create(&store, 1).unwrap();
    let identity = rotate_key(&store, &first_identity, 5).unwrap();
    let secret = signing_key(&store, &identity).unwrap();
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    drafts.set_identity(&identity).unwrap();
    let jobs = PublishJobs::new(&scratch.0.join("staging"));
    let publish = |drafts: &mut PublisherDrafts, version: &str| {
        publish_signed(&scratch, drafts, &jobs, (&identity, &store), version)
    };
    let listed = publish(&mut drafts, "1.0.0");
    let hidden = publish(&mut drafts, "2.0.0");
    let archived = publish(&mut drafts, "3.0.0");

    assert_eq!(drafts.catalog_title().unwrap(), "My courses");
    assert_eq!(
        drafts.set_catalog_title("  Ana's courses ").unwrap(),
        "Ana's courses"
    );
    assert!(drafts.set_catalog_title("").is_err());
    drafts
        .set_catalog_release(&listed.release.release_id, true)
        .unwrap();
    drafts
        .set_catalog_release(&archived.release.release_id, true)
        .unwrap();
    drafts
        .archive_release(&archived.release.release_id)
        .unwrap();
    assert!(matches!(
        drafts.set_catalog_release(&archived.release.release_id, true),
        Err(DraftServiceError::ReleaseArchived)
    ));
    let listings = drafts.catalog_listings().unwrap();
    assert!(
        listings
            .iter()
            .any(
                |listing| listing.release.release.release_id == hidden.release.release_id
                    && !listing.listed
            )
    );

    let retired = first_identity.current.clone();
    assert!(
        drafts
            .revoke_retired_key(&identity, &secret, &identity.current, 50, "x")
            .is_err()
    );
    assert!(
        drafts
            .revoke_retired_key(&identity, &secret, &retired, 50, "")
            .is_err()
    );
    drafts
        .revoke_retired_key(&identity, &secret, &retired, 50, "old laptop")
        .unwrap();
    drafts
        .revoke_retired_key(&identity, &secret, &retired, 60, "old laptop, sold")
        .unwrap();
    assert_eq!(drafts.issued_revocations().unwrap().len(), 1);

    let catalog = drafts
        .build_catalog(&releases, &identity, &secret, 100)
        .unwrap();
    let parsed = course_signing::parse_catalog(&catalog.to_bytes()).unwrap();
    let verified = course_signing::verify_catalog(&parsed).unwrap();
    assert_eq!(verified.root, identity.root_key().unwrap());
    assert_eq!(parsed.statement.title, "Ana's courses");
    let versions: Vec<&str> = parsed
        .statement
        .entries
        .iter()
        .map(|entry| entry.course_version.as_str())
        .collect();
    assert_eq!(versions, ["1.0.0"]);
    assert_eq!(parsed.statement.entries[0].magnet, listed.release.magnet);
    assert_eq!(parsed.statement.revocations[0].statement.revoked_at, 60);
    let stranger_store = MemoryStore::default();
    let stranger = signing_key(&stranger_store, &create(&stranger_store, 1).unwrap()).unwrap();
    assert!(matches!(
        drafts.build_catalog(&releases, &identity, &stranger, 1),
        Err(DraftServiceError::InvalidAttempt(_))
    ));
}
