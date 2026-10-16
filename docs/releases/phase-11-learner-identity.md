# Phase 11, slice 3: learner identity

Status: complete, with the manual checks below still to run. Each computer can now have a **learner key**: the key that licenses (slice 4) will be made for, kept in the operating system's keychain. There is no account and no password. Settings → Learner key creates it, shows its fingerprint, and saves or restores a passphrase-encrypted backup.

Decision record: [ADR 0017](../decisions/0017-paid-courses-and-licenses.md) decision 3.

## Delivered

- **New crate `learner-identity`:**
  - **The seed:** one random 32-byte seed in the credential store, under service `app.zeron.learner` and account `learner-key`.
  - **Derived keys:** HKDF-SHA256 (salt `zeron.learner-key.v1`) derives the Ed25519 signing key (info `signing`) and the X25519 encryption key (info `encryption`). This mapping is pinned by a test against an independent Python computation, which was itself checked against RFC 8032 test 1 and RFC 7748 section 6.1.
  - **Public data:** `LearnerIdentity { signingKey: "ed25519-…", encryptionKey: "x25519-…", createdAt }` is plain public data. `LearnerKeys` holds the secrets and prints only the public signing key.
  - **Operations:**
    - `create` refuses when the keychain already has a key.
    - `stored_identity` recovers the identity from the keychain.
    - `learner_keys` reports a missing or a different key.
  - **Backups:** `export_backup` and `restore_backup` write `zeron.learner-key-backup` version 1. The envelope is the same Argon2id and XChaCha20-Poly1305 one publisher backups use, with the public identity bound as associated data. Restoring refuses a backup of another format, one that was altered, or one larger than 16 KiB. It never replaces a different key already in the keychain, and restoring the same key twice is harmless.
- **`publisher-identity`:**
  - The passphrase envelope moved into a public `passphrase` module (`seal`, `open`, `MIN_PASSPHRASE_CHARS`), so learner backups share it.
  - `OsStore::open_service` opens the keychain under another service name.
  - Publisher backup files are byte-for-byte the same format. A backup written by the previous code is kept as a fixture and restores.
- **`learning-application`:** `learner_identity_record` and `set_learner_identity_record` keep the public identity as a Library setting. No schema change.
- **Desktop:**
  - `learner_key` module and commands `get_learner_key`, `create_learner_key`, `export_learner_key_backup { passphrase }` (save dialog), and `restore_learner_key_backup { passphrase }` (open dialog).
  - `LearnerKeyDto.status` is `none`, `ready`, `key_missing`, or `keychain_unavailable`. It carries the public keys, the fingerprint, the creation time, and a plain problem message. No secret crosses IPC.
  - **Recovery:** a key the keychain holds without a Library record, for example after a database reset, is recorded again. A recorded key the keychain lost is reported as `key_missing`, so it can be restored.
  - Error codes: `learner_key_exists`, `learner_key_missing`, `key_missing`, `keychain_unavailable`, `weak_passphrase`, `wrong_passphrase`, `backup_invalid`, `backup_unreadable`, and `export_failed`.
- **UI:**
  - **Settings → Learner key:** create; status with the reason when the key or keychain is unavailable; fingerprint and creation time; backup with a repeated passphrase of at least 12 characters; restore from a backup.
  - **Settings → Backup:** now says it holds none of your keys.
- **CI:** the real-keychain step runs the learner key test on Linux (GNOME Keyring), macOS, and Windows. The non-Linux step now runs in bash, so a failing first test is not hidden by a passing second one.

## Acceptance evidence

- **`learner-identity`** (5 unit tests):
  - created once, both keys derived, and `Debug` free of secrets;
  - the derivation is stable and the two keys are separate;
  - missing, different, and malformed keychain entries;
  - backup and restore across stores, including the wrong passphrase, the same key again, and refusing to replace another key;
  - swapped identities, a publisher-format file, and oversized files refused, leaving the keychain untouched.
- **Real keychain** (`tests/os_store.rs`, under its own test service so it never touches a real learner key): passed locally against this machine's Secret Service with `ZERON_KEYCHAIN_TESTS=1`. CI runs it on all three platforms.
- **`publisher-identity`:** the previous-format backup fixture restores, and all earlier tests pass.
- **Desktop command tests** (in-memory keychain and Library):
  - created once and described without the seed;
  - a lost database record is recovered and a lost key is reported;
  - backup refused without a key or with a weak passphrase;
  - restore on another computer, wrong passphrase, invalid file, and refusal to replace another key.
- **Vitest:** create and fingerprint (axe-core clean); passphrase rules and saving; the missing-key explanation, then a wrong passphrase, then a successful restore.
- **Repository gates:** all pass (Rust 413 tests, Vitest 215). `cargo deny check licenses bans` passes; the new crates are `learner-identity` and `x25519-dalek` 3.0.0 (BSD-3-Clause, on the same `curve25519-dalek` as `ed25519-dalek`).

## Manual verification (not yet run)

1. Settings → Learner key → "Create learner key". On Linux, `secret-tool search service app.zeron.learner` should list account `learner-key`. On macOS, Keychain Access should list `app.zeron.learner`; on Windows, Credential Manager should.
2. Save a backup, remove the keychain entry, and reopen Settings. It should say the keychain no longer has the key. Restore from the backup; the same fingerprint should return.
3. On a second computer or user account, restore the backup. The fingerprint should match.

## Current limitations

- **Nothing uses the key yet.** Licenses (slice 4) seal content keys to it, and issuer requests (slice 5) are signed with it.
- **No rotation.** Licenses will be bound to the key, so a new key means asking the issuer for new licenses (slice 7).
- **The key is created only on request.** Later slices create it automatically when the learner first buys or signs in.
- **One learner key per computer user account.** There is no way to keep several.
