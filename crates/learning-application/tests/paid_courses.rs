//! Paid lessons are sealed when published and open in the Library only with
//! the course version's content key (ADR 0017, Phase 11 slice 2).

use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use learning_application::{
    CancellationFlag, ContentKey, ContentKeys, CourseLibrary, DecryptingReader, DocumentError,
    ImportLimits, InfoHash, KeyRequest, LibraryError, NoContentKeys, ResourceAccess, StreamWait,
    TorrentImportStage, TorrentImportTimeouts, TorrentImports, TorrentSource, open_document,
    open_lesson_stream, read_local_document,
};
use publisher_application::{
    DistributionOptions, MetadataEdit, PublishJobs, PublisherDrafts, ReleaseSecrets, ReleaseSigner,
    StructureEdit, issue_license, release_content_key, stored_content_key,
};
use publisher_identity::MemoryStore;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use torrent_librqbit::{EngineConfig, LibrqbitEngine, PeerReachability};
use torrent_port::{NetworkProfile, SeedRequest, TorrentEngine, TransferState};

const COURSE_VERSION: &str = "1.0.0";
const TRANSCRIPT: &[u8] = b"WEBVTT\n\n00:00.000 --> 00:02.000\nsealed ownership lesson\n";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zeron-paid-courses-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_writable(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            make_writable(&path);
        } else if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}

/// The publisher's keychain as the Library sees it on the publishing computer.
struct StoreKeys(Arc<MemoryStore>);

impl ContentKeys for StoreKeys {
    fn content_key(&self, request: &KeyRequest<'_>) -> Option<ContentKey> {
        stored_content_key(self.0.as_ref(), request.course_id, request.version)
            .ok()
            .flatten()
    }
}

/// A learner's computer: licensed content keys open with its learner key.
struct LearnerKeys(Arc<MemoryStore>);

impl ContentKeys for LearnerKeys {
    fn content_key(&self, request: &KeyRequest<'_>) -> Option<ContentKey> {
        let keys = learner_identity::stored_keys(self.0.as_ref()).ok()??;
        let me = keys.signing().public_key().to_string();
        request
            .licensed
            .iter()
            .filter(|licensed| licensed.learner_key == me)
            .find_map(|licensed| {
                course_access::unwrap_content_key(
                    &licensed.wrapped_content_key,
                    &keys.encryption_secret_bytes(),
                    request.course_id,
                    request.version,
                )
                .ok()
            })
    }
}

struct Published {
    package: PathBuf,
    torrent: Vec<u8>,
    info_hash: String,
    video: Vec<u8>,
    preview: Vec<u8>,
    store: Arc<MemoryStore>,
    signer: ReleaseSigner,
    identity: publisher_identity::Identity,
    course_id: String,
}

/// A course with a free preview lesson and a paid lesson holding a
/// multi-chunk "video" and its transcript.
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end fixture: draft, sign, seal, publish"
)]
fn publish(root: &Path) -> Published {
    let mut drafts = PublisherDrafts::open_in_memory().unwrap();
    let draft_id = drafts.create().unwrap().draft.draft_id;
    let mut revision = 0;
    let mut edit = |drafts: &mut PublisherDrafts, edit: StructureEdit| {
        let stored = drafts.edit_structure(&draft_id, revision, edit).unwrap();
        revision += 1;
        stored.draft
    };
    let draft = edit(
        &mut drafts,
        StructureEdit::AddModule {
            title: "M".to_owned(),
        },
    );
    let module_id = draft.modules[0].id.clone();
    for title in ["Preview", "Paid"] {
        edit(
            &mut drafts,
            StructureEdit::AddLesson {
                module_id: module_id.clone(),
                title: title.to_owned(),
            },
        );
    }
    let lessons = drafts.draft(&draft_id).unwrap().unwrap().draft.modules[0]
        .lessons
        .iter()
        .map(|lesson| lesson.id.clone())
        .collect::<Vec<_>>();
    let draft = edit(
        &mut drafts,
        StructureEdit::SetLessonPaid {
            lesson_id: lessons[1].clone(),
            paid: true,
        },
    );
    assert!(draft.modules[0].lessons[1].paid);

    let preview: Vec<u8> = (0..5_000_u32).map(|i| (i % 7) as u8).collect();
    let video: Vec<u8> = (0..300_000_u32).map(|i| (i * 13 % 251) as u8).collect();
    let sources = [
        (lessons[0].as_str(), "preview.bin", preview.as_slice()),
        (lessons[1].as_str(), "lecture.bin", video.as_slice()),
        (lessons[1].as_str(), "lecture.vtt", TRANSCRIPT),
    ];
    for (lesson_id, name, bytes) in sources {
        let source = root.join(name);
        std::fs::write(&source, bytes).unwrap();
        drafts
            .attach_source(&draft_id, revision, lesson_id, &source)
            .unwrap();
        revision += 1;
    }
    drafts
        .update_metadata(
            &draft_id,
            revision,
            MetadataEdit {
                version: COURSE_VERSION.to_owned(),
                title: "Paid Course".to_owned(),
                publisher_name: "Zeron Tests".to_owned(),
                language: "en".to_owned(),
                ..MetadataEdit::default()
            },
        )
        .unwrap();
    let draft = drafts.draft(&draft_id).unwrap().unwrap().draft;
    assert!(draft.has_paid_content());

    let store = Arc::new(MemoryStore::default());
    let identity = publisher_identity::create(store.as_ref(), 1).unwrap();
    let signer = || {
        ReleaseSigner::new(
            identity.clone(),
            publisher_identity::signing_key(store.as_ref(), &identity).unwrap(),
            10,
        )
        .unwrap()
    };
    let release_signer = signer();
    let key = release_content_key(
        store.as_ref(),
        &draft.metadata.course_id,
        &draft.metadata.version,
    )
    .unwrap();
    let jobs = PublishJobs::new(&root.join("staging"));
    let job = jobs.begin(&draft).unwrap();
    let finished = jobs
        .run_with(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            ReleaseSecrets {
                signer: Some(&release_signer),
                content_key: Some(&key),
            },
            &mut |_| {},
        )
        .unwrap();
    assert!(finished.failure.is_none(), "{:?}", finished.failure);
    let releases = root.join("releases");
    let (record, _) = jobs.activate(&job.job_id, &mut drafts, &releases).unwrap();
    let directory = releases.join(&record.release.release_id);
    Published {
        package: directory.join("package"),
        torrent: std::fs::read(directory.join("release.torrent")).unwrap(),
        info_hash: record.release.info_hash,
        video,
        preview,
        signer: signer(),
        identity,
        course_id: draft.metadata.course_id.clone(),
        store,
    }
}

fn resource_id(library: &CourseLibrary, course_id: &str, kind: &str) -> String {
    let manifest = library.manifest(course_id).unwrap().unwrap();
    manifest
        .resources()
        .find(|resource| resource.path.ends_with(kind))
        .unwrap()
        .id
        .clone()
}

#[test]
fn a_paid_lesson_is_sealed_locked_without_its_key_and_opens_with_it() {
    let scratch = Scratch::new("folder");
    let published = publish(&scratch.0);
    let mut library =
        CourseLibrary::open(&scratch.0.join("library.sqlite3"), ImportLimits::default()).unwrap();
    library
        .import_local_folder(&published.package, &CancellationFlag::default())
        .unwrap();
    let course_id = library.courses().unwrap()[0].course_id.clone();
    let manifest = library.manifest(&course_id).unwrap().unwrap();
    assert!(manifest.access.is_some());

    let video_id = resource_id(&library, &course_id, "lecture.bin");
    let transcript_id = resource_id(&library, &course_id, "lecture.vtt");
    let preview_id = resource_id(&library, &course_id, "preview.bin");
    let keys = StoreKeys(Arc::clone(&published.store));

    // Without a key: the preview is free, paid resources are locked, and
    // their package bytes are ciphertext.
    let locked = |id: &str| {
        library
            .resource_access(&course_id, id, &NoContentKeys)
            .unwrap()
            .unwrap()
    };
    assert!(matches!(locked(&preview_id), ResourceAccess::Free));
    assert!(locked(&video_id).is_locked());
    assert!(locked(&transcript_id).is_locked());
    assert_eq!(
        library
            .locked_resources(&manifest, &NoContentKeys)
            .unwrap()
            .len(),
        2
    );
    assert!(
        library
            .locked_resources(&manifest, &keys)
            .unwrap()
            .is_empty()
    );

    let transcript = library
        .local_resource(&course_id, &transcript_id)
        .unwrap()
        .unwrap();
    let sha256 = manifest
        .resources()
        .find(|resource| resource.id == transcript_id)
        .unwrap()
        .sha256
        .clone();
    let sealed = read_local_document(&transcript.path, &sha256, transcript.byte_length).unwrap();
    assert_ne!(sealed, TRANSCRIPT);
    assert!(matches!(
        open_document(sealed.clone(), &locked(&transcript_id)),
        Err(DocumentError::Locked)
    ));

    // With the publisher's key: the transcript and the video open.
    let access = |id: &str| {
        library
            .resource_access(&course_id, id, &keys)
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        open_document(sealed.clone(), &access(&transcript_id)).unwrap(),
        TRANSCRIPT
    );
    let mut altered = sealed;
    altered[3] ^= 1;
    assert!(matches!(
        open_document(altered, &access(&transcript_id)),
        Err(DocumentError::Changed)
    ));

    let ResourceAccess::Unlocked(cipher) = access(&video_id) else {
        panic!("the paid video opens with its key");
    };
    assert_eq!(cipher.plaintext_length(), published.video.len() as u64);
    let video = library
        .local_resource(&course_id, &video_id)
        .unwrap()
        .unwrap();
    assert_eq!(video.byte_length, cipher.sealed_length());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let file = tokio::fs::File::open(&video.path).await.unwrap();
        let mut reader = DecryptingReader::new(file, cipher);
        reader.seek(SeekFrom::Start(150_000)).await.unwrap();
        let mut middle = vec![0; 70_000];
        reader.read_exact(&mut middle).await.unwrap();
        assert_eq!(middle, published.video[150_000..220_000]);
    });

    // Opened text indexes for search like any other.
    let indexed = library
        .index_text_resource(&course_id, &transcript_id, TRANSCRIPT)
        .unwrap();
    assert!(indexed);
    let preview = library
        .local_resource(&course_id, &preview_id)
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(preview.path).unwrap(), published.preview);
}

async fn engine(root: &Path, name: &str, allow_uploads: bool) -> Arc<LibrqbitEngine> {
    Arc::new(
        LibrqbitEngine::start(EngineConfig {
            profile: NetworkProfile::Loopback,
            state_directory: root.join(name),
            reachability: PeerReachability::default(),
            allow_uploads,
        })
        .await
        .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paid_lesson_streams_decrypted_from_a_torrent() {
    let scratch = Scratch::new("torrent");
    let published = publish(&scratch.0);
    let seeder = engine(&scratch.0, "seeder", true).await;
    let info_hash = seeder
        .seed(SeedRequest {
            torrent: published.torrent.clone(),
            content_root: published.package.clone(),
        })
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while seeder.transfer(&info_hash).unwrap().state != TransferState::Seeding {
        assert!(Instant::now() < deadline, "seeder never became ready");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let peer = seeder.listen_address().unwrap();

    let library = Arc::new(Mutex::new(
        CourseLibrary::open(&scratch.0.join("library.sqlite3"), ImportLimits::default()).unwrap(),
    ));
    let learner = engine(&scratch.0, "learner", false).await;
    let imports = TorrentImports::new(
        Arc::clone(&learner),
        Arc::clone(&library),
        scratch.0.join("courses"),
        ImportLimits::default(),
        TorrentImportTimeouts::default(),
    );
    let job = imports.begin().unwrap();
    let imported = imports
        .run(
            &job.job_id,
            TorrentSource::Magnet(format!("magnet:?xt=urn:btih:{}", published.info_hash)),
            vec![peer],
            &mut |_| {},
        )
        .await
        .unwrap();
    assert_eq!(imported.stage, TorrentImportStage::Imported, "{imported:?}");
    let course_id = imported.course_id.unwrap();

    let keys = StoreKeys(Arc::clone(&published.store));
    let (resource, access) = {
        let library = library.lock().unwrap();
        let video_id = resource_id(&library, &course_id, "lecture.bin");
        let resource = library
            .torrent_resources(&course_id)
            .unwrap()
            .into_iter()
            .find(|resource| resource.resource_id == video_id)
            .unwrap();
        assert!(
            library
                .resource_access(&course_id, &video_id, &NoContentKeys)
                .unwrap()
                .unwrap()
                .is_locked()
        );
        let access = library
            .resource_access(&course_id, &video_id, &keys)
            .unwrap()
            .unwrap();
        (resource, access)
    };
    let ResourceAccess::Unlocked(cipher) = access else {
        panic!("the paid video opens with its key");
    };
    let info_hash = InfoHash::parse(&resource.info_hash).unwrap();
    let file = open_lesson_stream(
        &*learner,
        &info_hash,
        resource.file_index,
        StreamWait::default(),
    )
    .await
    .unwrap();
    let mut reader = DecryptingReader::new(file, cipher);
    reader.seek(SeekFrom::Start(200_000)).await.unwrap();
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).await.unwrap();
    assert_eq!(tail, published.video[200_000..]);
    reader.seek(SeekFrom::Start(0)).await.unwrap();
    let mut head = vec![0; 1_000];
    reader.read_exact(&mut head).await.unwrap();
    assert_eq!(head, published.video[..1_000]);
}

fn import_folder(root: &Path, published: &Published) -> (CourseLibrary, String) {
    let mut library =
        CourseLibrary::open(&root.join("library.sqlite3"), ImportLimits::default()).unwrap();
    library
        .import_local_folder(&published.package, &CancellationFlag::default())
        .unwrap();
    let course_id = library.courses().unwrap()[0].course_id.clone();
    (library, course_id)
}

fn license_for(published: &Published, learner: &learner_identity::LearnerIdentity) -> Vec<u8> {
    let id = learner_identity::parse_learner_id(&learner.learner_id()).unwrap();
    issue_license(
        published.store.as_ref(),
        &published.signer,
        &published.course_id,
        &[COURSE_VERSION.to_owned()],
        &id.signing_key,
        &id.encryption_key,
    )
    .unwrap()
    .to_bytes()
}

#[test]
fn a_license_unlocks_a_paid_course_offline_for_its_learner_only() {
    let scratch = Scratch::new("license");
    let published = publish(&scratch.0);
    let (mut library, course_id) = import_folder(&scratch.0, &published);
    let manifest = library.manifest(&course_id).unwrap().unwrap();
    let video_id = resource_id(&library, &course_id, "lecture.bin");

    // This learner's computer has its own key and none of the publisher's.
    let learner_store = Arc::new(MemoryStore::default());
    let learner = learner_identity::create(learner_store.as_ref(), 1).unwrap();
    let learner_key: course_signing::PublicKey = learner.signing_key.parse().unwrap();
    let keys = LearnerKeys(Arc::clone(&learner_store));
    assert_eq!(library.locked_resources(&manifest, &keys).unwrap().len(), 2);

    let license = license_for(&published, &learner);
    let imported = library.import_license(&license, &learner_key).unwrap();
    assert_eq!(imported.course_id, course_id);
    assert_eq!(imported.versions, [COURSE_VERSION]);
    assert!(imported.unlocks_current && imported.new);
    assert!(!library.import_license(&license, &learner_key).unwrap().new);

    assert!(
        library
            .locked_resources(&manifest, &keys)
            .unwrap()
            .is_empty()
    );
    let ResourceAccess::Unlocked(cipher) = library
        .resource_access(&course_id, &video_id, &keys)
        .unwrap()
        .unwrap()
    else {
        panic!("the license unlocks the paid video");
    };
    let video = library
        .local_resource(&course_id, &video_id)
        .unwrap()
        .unwrap();
    let sealed = std::fs::read(video.path).unwrap();
    assert_eq!(cipher.open_all(&sealed).unwrap(), published.video);

    // Another learner's computer cannot use it, even with the file.
    let other_store = Arc::new(MemoryStore::default());
    let other = learner_identity::create(other_store.as_ref(), 1).unwrap();
    let other_key: course_signing::PublicKey = other.signing_key.parse().unwrap();
    assert!(matches!(
        library.import_license(&license, &other_key),
        Err(LibraryError::License(message)) if message.contains("another learner")
    ));
    let other_keys = LearnerKeys(other_store);
    assert_eq!(
        library
            .locked_resources(&manifest, &other_keys)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn forged_altered_and_revoked_licenses_do_not_unlock() {
    let scratch = Scratch::new("license-refused");
    let published = publish(&scratch.0);
    let (mut library, course_id) = import_folder(&scratch.0, &published);
    let manifest = library.manifest(&course_id).unwrap().unwrap();
    let learner_store = Arc::new(MemoryStore::default());
    let learner = learner_identity::create(learner_store.as_ref(), 1).unwrap();
    let learner_key: course_signing::PublicKey = learner.signing_key.parse().unwrap();
    let keys = LearnerKeys(Arc::clone(&learner_store));
    let license = license_for(&published, &learner);

    // Signed by someone other than the course's publisher.
    let impostor_store = MemoryStore::default();
    let impostor = publisher_identity::create(&impostor_store, 1).unwrap();
    let impostor_signer = ReleaseSigner::new(
        impostor.clone(),
        publisher_identity::signing_key(&impostor_store, &impostor).unwrap(),
        10,
    )
    .unwrap();
    let forged = impostor_signer
        .license(
            &course_id,
            &learner_key,
            Vec::from([course_signing::LicensedKey {
                course_version: COURSE_VERSION.to_owned(),
                wrapped_content_key: "00".repeat(80),
            }]),
        )
        .to_bytes();
    assert!(matches!(
        library.import_license(&forged, &learner_key),
        Err(LibraryError::License(message)) if message.contains("another publisher")
    ));

    // Altered after signing.
    let altered = String::from_utf8(license.clone())
        .unwrap()
        .replace(COURSE_VERSION, "9.9.9")
        .into_bytes();
    assert!(matches!(
        library.import_license(&altered, &learner_key),
        Err(LibraryError::License(_))
    ));
    assert!(matches!(
        library.import_license(b"not json", &learner_key),
        Err(LibraryError::License(_))
    ));

    // A valid license stops unlocking once its signing key is revoked.
    library.import_license(&license, &learner_key).unwrap();
    assert!(
        library
            .locked_resources(&manifest, &keys)
            .unwrap()
            .is_empty()
    );
    let secret =
        publisher_identity::signing_key(published.store.as_ref(), &published.identity).unwrap();
    library
        .add_key_revocation(&course_signing::RevocationRecord {
            publisher_id: published.identity.root.clone(),
            rotations: Vec::new(),
            revocation: course_signing::revoke(
                &secret,
                &published.identity.current_key().unwrap(),
                5,
                "laptop stolen",
            ),
        })
        .unwrap();
    assert_eq!(library.locked_resources(&manifest, &keys).unwrap().len(), 2);
    assert!(matches!(
        library.import_license(&license, &learner_key),
        Err(LibraryError::License(message)) if message.contains("laptop stolen")
    ));
}
