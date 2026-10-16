# Phase 9: the Library verifies publishers

Status: complete. When a course arrives from a folder, a `.torrent`, or a magnet, the Library verifies its publisher signature. The first signed version pins the course to its publisher, so a newer version from anyone else, or an unsigned one, is refused and the learner's copy is kept. The course page shows "Verified publisher" with the fingerprint, or "Unsigned course".

## Delivered

- **`learning-storage` schema v13:**
  - New tables:
    - `course_signatures` holds the verified signature of each course's current version;
    - `course_pins` holds the publisher each course is pinned to;
    - `key_revocations` holds signed revocation statements, which catalogs will supply.
  - **Saving:** `save_imported_course_trusted` and `save_torrent_course_trusted` record the signature and a first pin in the same transaction as the course.
  - **Queries:** `course_pin`, `forget_course_pin`, `course_signature`, `revocation_statements`, `add_revocation_statement`.
- **`learning-application`:**
  - **Reading the signature:**
    - `import_local_course` now returns the SHA-256 of the exact `course.json` and the bytes of `zeron-signature.json`. The file must be a regular file, not a link, and at most 64 KiB.
    - Torrent import accepts at most one `zeron-signature.json` at the torrent root. It fetches it with `course.json`, leaves it out of the inventory cross-check, and verifies it before saving.
    - The download selection keeps both files, so a learner who shares the course serves the signature to others.
  - **Trust rules** (`CourseLibrary::course_trust`):
    - A signature must verify against the manifest and its `publisherId`, through the rotation chain.
    - A key revoked at the signing time is refused, with the publisher's reason.
    - A pinned course accepts only versions signed by its publisher; a different signer or an unsigned version gives `PublisherChanged`.
    - The first signed version pins the course.
  - **Queries and actions:** `course_publisher`, `forget_course_publisher` (removes only the pin), and `add_key_revocation`.
  - New errors: `LibraryError::Signature` and `PublisherChanged { pinned, found }`, whose message names both fingerprints.
- **Commands:**
  - `get_course_detail` adds `publisher { status: verified | signed | pinned_unsigned | unsigned, fingerprint, publisherId, signedAtUnixSeconds }`.
  - `forget_course_publisher { courseId }`.
  - Folder imports and torrent import jobs fail with `signature_invalid` or `publisher_changed`.
- **UI** (`PublisherBadge` on the course page):
  - "Verified publisher" shows the fingerprint, explains that updates must come from this publisher, and offers "Forget publisher…".
  - That confirmation explains that the next signed import will be trusted whoever signed it, and that notes and progress stay.
  - Unsigned courses carry a warning.

## Compatibility

- **Storage:** migrating v12 to v13 adds the three tables. Courses imported before this build have no pin. Their next signed version pins them.
- **IPC:** additive.
- **Packages:** unsigned packages import exactly as before.

## Acceptance evidence

- **Storage test:** signatures follow each saved version, a pin is set once and never replaced by a save, forgetting removes only the pin, and an untrusted save clears the signature.
- **Application tests** (`tests/signed_import.rs`), publishing through the real publisher pipeline:
  - **Folder imports:**
    - a signed 1.0.0 pins its publisher;
    - an impostor-signed 1.1.0 with the same `courseId` is refused with both fingerprints;
    - an unsigned 1.2.0 is refused, and the Library keeps 1.0.0;
    - a genuine 1.3.0 imports;
    - after "forget publisher", the impostor's version imports and pins the impostor.
  - **Tampering and revocation:** a signature file with a changed `signedAt` is refused and nothing is saved. A revocation from 50 refuses a release signed at 100, naming the reason.
  - **Real loopback torrents:** a magnet import fetches and verifies the signature and pins the publisher, and the selection keeps both metadata files. The impostor's magnet fails with `publisher_changed` naming the pinned fingerprint, and the course stays at 1.0.0.
- **Vitest:** the verified badge with confirmation before forgetting; the unsigned warning; the Library course page showing the badge.
- All repository gates pass.

## Manual verification

1. Publish a signed release (Publish → Identity, then publish) and import its magnet in another Zeron. The course page should say "Verified publisher" with the same fingerprint that Publish → Identity shows.
2. On a second publisher computer with a different identity, publish a version of the same course. A copy of the publisher database would do. Importing it should be refused, naming both fingerprints.

## Current limitations

- Trust on first use: the first version imported is trusted as it is. Compare fingerprints out of band.
- There is no way to remove a course from the Library. "Forget publisher" is the deliberate escape hatch.
- Revocations arrive only through catalogs, in the next slice.
