# Phase 9: publishing signed releases

Status: complete. Publish → Identity creates the publisher's signing identity in the operating system's keychain, backs it up, restores it, and rotates its key. Once an identity exists, every new release is signed: `course.json` names the publisher (`publisherId`), and `zeron-signature.json` signs the package. Release details show the signature's fingerprint.

## Delivered

- **`course-publisher`:** `stage_signed_package` and `PackageSigning`. The signing hook receives the final manifest and its SHA-256 and returns the signature file's bytes, so the crate stays free of crypto. A resource that would take the signature file's name is refused. `StagedPackage::signature_file` records it.
- **`publisher-application`:**
  - **`ReleaseSigner`** holds an identity and its current secret for one preparation. It refuses a key that does not belong to the identity, and its `Debug` shows only public keys.
  - **Signed preparation:** `stage_signed_draft` and `PublishJobs::run_signed`, and `StagingReceipt::signed`.
  - **Package checks** (`StagedAttempt::open_package`) accept `zeron-signature.json` only when it verifies against `course.json` and its `publisherId`. They include it in the torrent's file list, so the info hash covers it. A changed signature file makes activation, seeding, and export refuse the package.
  - **Settings:** `PublisherDrafts::identity`, `set_identity` (public keys only, chain verified), `sign_releases` (on by default) and `set_sign_releases`, and `release_signature`.
- **Commands** (`publish::identity`):
  - `get_publisher_identity`, `create_publisher_identity`, `rotate_publisher_identity`, `set_sign_releases`;
  - `export_identity_backup { passphrase }` through a save dialog, and `restore_identity_backup { passphrase }` through an open dialog;
  - `IdentityDto.status` is `none`, `ready`, `key_missing`, or `keychain_unavailable`, with a plain reason. No secret crosses IPC.
  - **Opening the keychain:** the keychain is opened on first use, so a missing Secret Service never delays startup.
  - **When preparation starts:**
    - With an identity and signing on, preparation needs the key. If the keychain or key is unavailable, it stops with `keychain_unavailable` or `key_missing` rather than quietly publishing unsigned.
    - Rotation records the new identity first and then removes the retired key.
    - Restoring a backup of another identity is refused with `identity_exists`.
  - Error codes: `identity_exists`, `identity_missing`, `weak_passphrase`, `wrong_passphrase`, `backup_invalid`, `backup_unreadable`, `export_failed`.
- **DTOs:**
  - `get_release_details` adds `signature { publisherId, fingerprint, signingKey, signedAtUnixSeconds }`, or `null` for an unsigned release.
  - The prepared release adds `signed`.
- **UI:**
  - **Publish → Identity:** create; status with the reason when the key or keychain is unavailable; fingerprint and publisher ID; the "Sign new releases" toggle with a warning when off; backup with a repeated passphrase of at least 12 characters; restore; and rotation with a confirmation that recommends a new backup.
  - **Elsewhere:** the Prepare panel shows "Signed with your publisher identity", and release details show "Signed · fingerprint" or "Unsigned".

## Compatibility

- **Packages:** signed packages add one file. Older Zeron builds refuse them as torrents, because the file is not described by `course.json` (ADR 0014). Unsigned releases are unchanged.
- **Storage and IPC:** no schema change; the identity is a JSON setting holding public data. The IPC changes are additive.

## Acceptance evidence

- **`course-publisher` test:**
  - a signed package names the publisher in `course.json`;
  - the hook sees the final manifest and digest;
  - the signature file is written;
  - unsigned staging is unchanged;
  - a reserved resource name is refused and leaves nothing behind.
- **`publisher-application` tests** (`tests/signing.rs`):
  - a signed release names the publisher and verifies independently;
  - the torrent lists the signature file;
  - the Library's local import still accepts the package;
  - after a rotation the next release verifies with the same root and the new signing key;
  - a signer with the wrong key is refused;
  - a changed signature file stops activation.
- **Command tests** (`publish::identity`, in-memory keychain):
  - creation is refused the second time, and the description carries the fingerprint and no secret;
  - rotation removes the retired key, and turning signing off gives no signer;
  - backups use a passphrase of at least 12 characters, a wrong passphrase is refused, and restoring works on a fresh computer but not into another identity;
  - a missing key reports `key_missing`.
- **Vitest:** create; missing key and locked keychain; passphrase rules and backup; wrong passphrase and restore; rotation with confirmation and the signing toggle; the signed prepare panel; the release details fingerprint.
- The real-keychain test from the previous slice covers the OS stores on Linux (locally and in CI). The macOS and Windows CI results are recorded in `phase-9-complete.md`.
- All repository gates pass.

## Manual verification

1. Publish → Identity → "Create signing identity". On macOS, Keychain Access should list `app.zeron.publisher`; on Windows, Credential Manager → Windows Credentials should list it.
2. Save a backup, then prepare and publish a release. The Prepare panel and the release details should both say it is signed, with the same fingerprint.
3. Rotate the key and publish another version. Both releases should show the same fingerprint, which is the identity's.

## Current limitations

- The publisher CLI publishes unsigned releases only.
- The Library does not verify signatures yet; that is the next slice.
- Revocations are created with catalogs, in a later slice.
