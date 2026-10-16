# Phase 11, slice 4: offline licenses

Status: complete, with the manual checks below still to run. A paid course now unlocks on a learner's computer from a **license file**, verified with no network.

- **Learner side:** the learner copies their learner ID from Settings and sends it to the publisher. Opening a locked lesson offers **Import license…**.
- **Publisher side:** Publish → the paid release's details → "License a learner" saves a license for that ID.
- **Binding:** the license unlocks every released paid version for that learner's key only.
- **Revocation:** it stops working if the publisher revokes the key that signed it.

Decision record: [ADR 0017](../decisions/0017-paid-courses-and-licenses.md) decisions 4 and 5, with this slice's changes recorded there.

## Delivered

- **`course-signing`:**
  - **Statements:** a license `zeron.license.v1` `{ courseId, learnerKey, keys: [{ courseVersion, wrappedContentKey }], issuerKey, issuedAt }`, and a delegation `zeron.issuer-delegation.v1` `{ issuerKey, issuerUrl, validFrom, delegatedBy }`. Both use ADR 0014's canonical, domain-separated encoding.
  - **File:** `LicenseFile` `{ format: "zeron.license", version: 1, publisherId, rotations, delegation?, license }`.
  - **Functions:** `sign_license`, `sign_delegation`, `license_file`, `parse_license`, and `verify_license`.
  - **Parse limits:** at most 128 KiB and 256 versions, no duplicate versions, and an `https://` issuer address of at most 2 KiB.
  - **Verification:**
    - the file names the pinned publisher;
    - the chain verifies, and no rotation was made by a key revoked by then;
    - the license is for this learner;
    - the issuer is either a chain key, or delegated by a chain key that was not revoked at `validFrom`, with the license issued after the delegation;
    - the signature verifies.

    A revoked issuer key is reported with its reason.
  - **Revoking delegated keys:** `delegate_revoked_by_records` accepts a revocation of a delegated (issuer) key when any key of the publisher's verified chain signed it.
- **`course-access`:** `wrap_content_key` and `unwrap_content_key`, using HPKE base mode with X25519, HKDF-SHA256 and ChaCha20-Poly1305 (`hpke` 0.14.1). `info` is `zeron.license.v1`, and the associated data binds the course ID and version. A wrapped key is hex of the 32-byte encapsulated key plus the 48-byte sealed key.
- **`learner-identity`:**
  - Learner IDs: `LearnerIdentity::learner_id()` writes `zeron-learner:v1:<signing hex>:<encryption hex>`, and `parse_learner_id` reads it.
  - `stored_keys` reads whatever learner key the keychain holds, and `LearnerKeys::encryption_secret_bytes` exposes the X25519 secret for unwrapping.
- **`learning-storage`:** schema 17 adds the `licenses` table: the kept files, indexed by course. It has no foreign key, so licenses survive course removal like other learner data.
- **`learning-application`:**
  - **Import:** `CourseLibrary::import_license`. It refuses files for courses not in the Library, courses without paid lessons, unsigned courses, and anything that does not verify. It keeps each file once.
  - **Lookup:** `licensed_content_keys` re-verifies every kept license against the pinned publisher and current revocations whenever it is used.
  - **The `ContentKeys` port:** it now receives a `KeyRequest` carrying those licensed keys, so it never calls back into the locked Library. `locked_resources` became a Library method.
  - **New error:** `LibraryError::License`.
- **`publisher-application`:**
  - `issue_license` wraps the content key of every released version this keychain holds and signs the result with the publisher's current key, carrying its rotation chain.
  - `PublisherDrafts::released_versions` and `release_is_paid` support it.
- **Desktop:**
  - **Content keys:** `content_keys::LibraryContentKeys` tries the publisher's keychain first (the publishing computer), then opens licensed keys with this computer's learner key.
  - **Commands:**
    - `choose_and_import_license` (open dialog), with codes `license_invalid`, `learner_key_missing`, and `license_unreadable`;
    - `issue_learner_license { courseId, learnerId }` (save dialog), with codes `learner_id_invalid`, `identity_missing`, and `license_failed`;
    - `LearnerKeyDto.learnerId`;
    - `ReleaseDetailsDto.paid`.
  - **Publishing:** preparing a draft with paid lessons now requires signing (`signing_required`).
- **UI:**
  - **Library:** the locked-lesson panel has **Import license…**. It reports refusals, and a license for other versions. On success the course reloads unlocked.
  - **Settings → Learner key:** shows the learner ID with Copy.
  - **Publish:** a paid release's details have "License a learner".
- **Hardening:** a mutation test checks that a changed license never verifies as the original, and a new `license` fuzz target is added to the nightly fuzz workflow.

## Compatibility

- **Library database:** schema 16 → 17 adds one table. Older builds refuse the newer database, as for every schema change.
- **Publishing:** paid releases from slice 2 that were published unsigned cannot be licensed. Publish them again signed, as a new version.
- **IPC:** changes are additive.

## Acceptance evidence

- **`course-signing`:**
  - **License unit tests (5):**
    - publisher-signed licenses verify for their learner only;
    - other publishers and altered statements are refused;
    - a rotated key signs through its chain, and a stranger does not;
    - a delegated issuer signs only after its delegation and only as the delegated key, and self-delegation is refused;
    - revoked issuers are reported, a forged issuer revocation is ignored, and a delegation by an already-revoked key is refused;
    - malformed files are refused.
  - **Mutations:** 4,000 mutated license files never verify as a changed statement.
- **`course-access`:** a wrapped key opens only for its learner, course and version; altered and truncated values are refused; wrapping is randomized.
- **`learner-identity`:** learner IDs round-trip and reject variants; stored keys match the identity.
- **`learning-application`** (`tests/paid_courses.rs`, through the real publish job, now signed):
  - a hand-issued license imports and unlocks the paid video offline for its learner, decrypting it byte for byte;
  - importing it again is reported as already imported;
  - another learner's computer refuses it and stays locked;
  - an impostor publisher's license, an altered file, and garbage are refused;
  - after the signing key is revoked, the same license stops unlocking and is refused on import.

  These tests also prove that `x25519-dalek` keys interoperate with `hpke`.
- **Desktop unit tests:**
  - licensed keys open only with the learner key they were sealed to;
  - the publishing computer opens its own releases;
  - license import needs a learner key and refuses bad files without naming paths;
  - issuing needs a learner ID, an identity, and a paid release with a key here.
- **Vitest:**
  - Copy learner ID.
  - Import license: a refusal, then an unlock reloads the course.
  - Publish: "License a learner" appears only on paid releases, reports an invalid ID, and trims and sends a valid one.
- **Repository gates:** all pass (Rust 428 tests, Vitest 218). `cargo deny check licenses bans` passes; the new crate is `hpke` 0.14.1 (MIT/Apache-2.0). The fuzz target type-checks.

## Manual verification (not yet run)

1. **Publisher computer:** create a signing identity, publish a course with a paid lesson, and confirm "License a learner" shows in the release details.
2. **Second computer or user account:** import the course by magnet or folder; the paid lesson is locked. Create the learner key in Settings and copy the learner ID.
3. **Back on the publisher computer:** paste the ID, save the license, and move the file over.
4. **On the learner side:** open the locked lesson and choose Import license… with the network off. The lesson unlocks, plays and seeks.
5. **Another learner account:** importing the same file is refused with "another learner".

## Current limitations

- **Licenses are made and sent by hand.** The license server (slices 5–7) automates this after payment.
- **A license covers the versions released when it was issued.** Later versions need a new license, which slice 5's issuer will hand out on request.
- **Licenses for courses not yet in the Library are refused.** Import the course first.
- **There is no list of imported licenses in the UI,** and no way to remove one.
- **A refund cannot revoke a license already delivered** (ADR 0017 decision 9). Revoking the signing key would lock every license it signed.
