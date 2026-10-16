# Phase 9: signing foundation and three-platform CI

Status: complete. This first Phase 9 slice adds the signature formats and the publisher's key handling (ADR 0014), and a CI matrix that proves them on Linux, macOS, and Windows. Publishing signed releases, verifying them in the Library, and catalogs follow in the next slices.

## Delivered

- **`course-signing`** (new, pure):
  - **Keys:** `PublicKey` is written `ed25519-<64 hex>`, and parsing refuses invalid curve points and upper case. `SecretKey` never prints its bytes. `fingerprint` gives `XXXX-XXXX-XXXX-XXXX`.
  - **Canonical statements:** domain-separated compact JSON for releases, rotations, revocations, and catalogs. Verification uses strict Ed25519.
  - **Rotation chains:** `rotate` and `verify_chain`. Each rotation must be signed by the key it retires and continue the chain, with no cycles, no going back in time, and at most 32 rotations.
  - **Revocations:** `revoke` and `revoked`. A revocation counts only when signed by the revoked key or a later key of the chain, and only from its time on.
  - **Release signatures** (`zeron-signature.json`): `sign_release`, `parse_release_signature` (64 KiB bound, strict), and `verify_release`, which checks course, version, manifest digest, the chain from `publisherId`, and that no rotation was made by an already revoked key. A revoked signing key is reported in `VerifiedRelease::revoked`.
  - **Catalogs:** `sign_catalog`, `parse_catalog` (1 MiB, 1,000 entries, well-formed IDs, hashes, and magnets matching their info hash, no control characters), and `verify_catalog`.
- **`publisher-identity`** (new):
  - **`Identity`** holds the root key, current key, creation time, and rotation chain. It is public data and verifies its own chain.
  - **Key lifecycle:**
    - `create` generates a key from the OS random source.
    - `signing_key` loads the key and refuses a missing or mismatched one.
    - `rotate_key` and `forget_key` order a rotation so that a crash keeps both keys.
    - Keys are stored per public key under service `app.zeron.publisher`.
  - **`OsStore`** uses `keyring-core` with the macOS Keychain, the Windows Credential Manager, or the Secret Service on Linux. Error messages never include secrets.
  - **`MemoryStore`** is for tests.
  - **Backups:** `export_backup` and `restore_backup` use Argon2id (64 MiB, 3 passes) and XChaCha20-Poly1305, with the identity bound as associated data. A passphrase must be at least 12 characters. Restoring caps the cost parameters and checks that the key matches its identity.
- **CI** (`.github/workflows/ci.yml`): Ubuntu 24.04, macOS 15, and Windows 2025 run clippy, `cargo test --workspace`, the frontend tests, typecheck, and the real-keychain test. On Linux that test runs under `dbus-run-session` with an unlocked GNOME Keyring.
- **ADR 0014:** signing, key storage, trust on first use, revocation, and the catalog.

## Dependencies added

All are pinned in the workspace:
- `ed25519-dalek 3.0.0`
- `keyring-core 1.0.1`, with platform-specific stores:
  - `apple-native-keyring-store 1.0.2` (macOS only)
  - `windows-native-keyring-store 1.1.0` (Windows only)
  - `dbus-secret-service-keyring-store 1.0.1` (Linux and the BSDs only)
- `argon2 0.6.0`
- `chacha20poly1305 0.11.0`
- `zeroize 1.9.1` (already in the lockfile)

## Acceptance evidence

- **`course-signing`:** 13 unit tests:
  - text keys and lookalikes;
  - signatures bound to their key and message;
  - chains with skips, tampering, impersonation, going backwards, and cycles;
  - revocation time and signer rules;
  - release round trip, rotation, revoked rotations, every mismatch, and strict parsing;
  - catalog round trip, tampering, a claimed key, a self-revoked key, malformed entries, and bounds.
- **`publisher-identity`:** 4 unit tests (lifecycle and rotation, mismatched stored key, backup round trip, refused passphrases and tampered or costly backups).
- **Real keychain** `the_platform_keychain_holds_rotates_and_forgets_signing_keys` on 2026-10-16:
  - It passed locally against GNOME Keyring on Linux.
  - CI runs it on macOS and Windows; the first runs are linked from the commit.
- All repository gates pass.

## Current limitations

- Nothing signs releases yet; the next slice does.
- Linux needs a running Secret Service. This affects Linux only.
