# Phase 11, slice 2: encrypted packages

Status: complete, with the manual checks below still to run. A publisher can mark lessons as **Paid** in the Builder. Publishing seals every file of a paid lesson under a new per-version content key, kept in the operating system's keychain. The Library opens paid lessons only where that key is available; everywhere else they show as locked, and the free preview lessons stay open. Licenses (slice 4) will supply keys to other learners; until then, only the computer that published a paid course can open its paid lessons.

Decision record: [ADR 0017](../decisions/0017-paid-courses-and-licenses.md). Format choices come from the [spike](../research/paid-courses-spike-results.md). The wire format is in the [manifest specification](../specifications/course-manifest.md#paid-courses-schema-version-2).

## Delivered

- **New crate `course-access`:** the `zeron.chunked-xchacha20poly1305.v1` format.
  - `ContentKey` (zeroized on drop, never printed) and `ResourceCipher` (HKDF-SHA256 per resource; XChaCha20-Poly1305 per 65,520-byte chunk; associated data binding the resource ID, the chunk index, and the final chunk).
  - `seal` reads exactly the declared length, so a source that grew or shrank is refused. `open_all` opens a whole small resource.
  - `DecryptingReader`: a seekable `AsyncRead + AsyncSeek` plaintext view that keeps one 64 KiB chunk and never returns bytes from a chunk that failed authentication.
  - The crate does no file or network I/O of its own.
- **`course-format` (manifest schema version 2):**
  - Adds `access { model: "paid" }` and per-resource `encryption { scheme, chunkSize, plaintextLength }`. `byteLength` and `sha256` describe the sealed bytes.
  - Validation accepts versions 1 and 2, requires the version the content needs (free courses stay 1), requires `access` exactly when something is encrypted, and checks the scheme, the chunk size, and the sealed length.
  - The generated contract is `course-manifest-v2.schema.json`; the v1 file is kept unchanged. A new fixture is `valid-paid-course.json`.
  - Helpers: `Resource::content_length`, `CourseManifest::resources`, `required_schema_version`, and `sealed_byte_length`.
- **`course-publisher`:**
  - `DraftLesson::paid`. Drafts saved before this slice load as free, and free lessons serialize exactly as before.
  - `set_lesson_paid` and `has_paid_content`.
  - `stage_package_with` and `PackageOptions { signing, content_key }`. Paid lessons are sealed while copied, and the hash covers the sealed bytes. Progress and limits count package bytes. A paid lesson without a key fails with `ContentKeyRequired` and leaves nothing behind. Sealing under one key is deterministic, so the same draft and key give the same manifest.
- **`publisher-application`:**
  - `StructureEdit::SetLessonPaid`.
  - `ReleaseSecrets { signer, content_key }`, `stage_draft_with`, and `PublishJobs::run_with`. The failure code `content_key_required` is added, and `StagingReceipt::paid`.
  - Content keys: `release_content_key` creates a random 256-bit key on first use and saves it in the publisher's credential store, account `content-key:<courseId>@<version>`. A retry of the same course version reuses the key. `stored_content_key` reads it back. Keys never enter SQLite, IPC, or logs.
  - `draft_from_release` refuses a paid release ("a paid release cannot be reopened as a draft yet") rather than reuse sealed files as sources.
- **`learning-application`:**
  - The `ContentKeys` port, with `NoContentKeys`.
  - `CourseLibrary::resource_access`, which answers `Free`, `Unlocked(cipher)`, or `Locked`.
  - `locked_resources`, `paid_content_unlocked`, and `open_document`, plus the new `DocumentError::Locked`.
- **Desktop:**
  - The Library's content keys come from Publish's keychain (`PublishedContentKeys`). A missing or failing keychain reads as "no key".
  - **Playback:** `register_media` refuses a locked resource with `resource_locked`. It serves an unlocked one through the gateway as a decrypting stream with the plaintext length: `SealedFileMedia` for local courses (the file length is checked against the sealed length), and `TorrentMedia` with a cipher on top of the verified torrent stream.
  - **Documents:** `read_document`, `read_text_resource`, and `read_transcript` decrypt unlocked resources and refuse locked ones.
  - **Search:** background text indexing skips locked lessons and indexes unlocked ones as plaintext.
  - **Publishing:** preparation creates or reuses the content key for a draft with paid lessons and fails, rather than publishing unsealed, when the keychain is unavailable.
  - **IPC:**
    - Library `CourseDetailDto` adds `paid` and `paidUnlocked`, `LessonDto` adds `paid` and `locked`, and `ResourceDto` adds `locked`. Its `byteLength` is now the readable (plaintext) length.
    - Publish `LessonDto` adds `paid`, and `StructureEditInput` adds `setLessonPaid`.
- **UI:**
  - **Builder:** each lesson has a Free/Paid toggle (`aria-pressed`).
  - **Library contents:** a locked lesson shows a lock instead of its materials. Opening it shows "<title> is a paid lesson" with an explanation, and requests no media. Free previews play as before.

## Compatibility

- **Free courses:** unchanged. They are still schema version 1 and byte-identical, so older builds import them.
- **Paid packages:** schema version 2. Older builds refuse them, because v1 rejects unknown fields.
- **Data and IPC:** no SQLite migration. Draft snapshots gain an optional `paid` field. IPC changes are additive, and the new TypeScript fields are optional ("absent means false").

## Acceptance evidence

- **`course-access`** (10 tests):
  - round trips at every chunk boundary and through arbitrary seeks;
  - a known answer computed independently with a pure-Python XChaCha20-Poly1305 and HKDF (checked against draft-irtf-cfrg-xchacha test vector A.3.1);
  - refusals of a flipped bit, swapped chunks, a chunk spliced from another resource, truncation at a chunk boundary, a wrong key, and a wrong length;
  - a tampered chunk fails the read without returning bytes;
  - keys never print;
  - constants agree with `course-format`.
- **`course-format`:** the paid fixture round trips; free courses must stay version 1; each paid invariant is refused when broken; the v1 contract file is kept.
- **`course-publisher`:**
  - a paid lesson is sealed while the preview stays plain;
  - the sealed file opens to the source under the key, and the plaintext does not appear in the package;
  - progress ends complete;
  - the package passes the Library's independent import verification;
  - no key means `ContentKeyRequired` and no directory;
  - sealing is deterministic;
  - the paid flag edits and old snapshots load.
- **`publisher-application`:** one key per course version, reused on retry; malformed keychain entries are refused.
- **`learning-application`** (`tests/paid_courses.rs`, through the real publish job):
  - **Folder import:** the preview is free and paid resources are locked without a key. The sealed transcript differs from its source, opens with the key, and refuses altered bytes. The paid "video" decrypts at an arbitrary range from the local file.
  - **Torrent:** a learner engine imports the paid course by magnet from a local seeder. The paid video is locked without a key, and with it streams decrypted through `open_lesson_stream`, seeking forward and back.
- **Vitest:** the Builder's Free/Paid toggle edits round-trip. In the Library, a locked lesson shows the lock and the explanation and requests no media, and the free preview still plays. axe-core finds no violations.
- **Repository gates:** all pass on the tree rebased onto the YouTube playlist commit (Rust 402 tests, Vitest 212). `cargo deny check licenses bans` passes; the new crates are `course-access`, `hkdf` 0.13, and `hmac`.

## Manual verification (not yet run)

1. In Publish, create a course with two lessons, attach a real MP4 and a `.vtt` to the second, and mark it **Paid**. Prepare and publish. On Linux, `secret-tool search service app.zeron.publisher` should list `content-key:<courseId>@<version>`.
2. Import the release folder in the Library on the same computer. The paid lesson should play and seek normally in the WebKitGTK window, with its transcript.
3. Remove the keychain entry, or import on another computer. The paid lesson should show the lock and "is a paid lesson", the preview should still play, and `release/package/resources/...` should hold unreadable bytes.
4. Repeat step 2 for the release's magnet on a second Zeron instance on Loopback (with the key present).

## Current limitations

- **No licenses yet.** Only the publishing computer can open paid lessons; slice 4 adds licenses. Each course version has one content key, which never leaves the publisher's keychain in this slice.
- **Paid releases cannot be reopened as a draft.** Re-encrypting from the sealed package will come later.
- **The publisher CLI** publishes free lessons only. A draft with paid lessons fails there with `content_key_required`.
- **`access`** carries only `model`. The issuer URL and delegation arrive with slices 5 and 8, as an extension to schema version 2.
- **Removing a content key** from the keychain locks that version on the publishing computer too. Back up the keychain; there is no key export yet.
