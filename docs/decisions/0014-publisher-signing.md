# ADR 0014: Publisher signing, key storage, and the signed catalog

Status: accepted (2026-10-16). Answers roadmap approval question 4: the user wants publisher signing now, working on Windows and macOS without limitation.

## Context

A torrent info hash proves that bytes were not changed, not who published them. Anyone can repackage a course, keep its `courseId`, and share the magnet. The Library would then replace the learner's copy in place, because courses are keyed by `courseId`, and the learner's notes would attach to the impostor. The roadmap's Phase 9 also asks for a catalog with discovery and an explicit publication confirmation. The headless seed server was removed (Phase 8), so no Zeron server exists to host one.

## Decision

1. **Algorithm and encoding.** Ed25519, through `ed25519-dalek` (pure Rust, identical on every platform).
   - Keys are written `ed25519-<64 lowercase hex>`, which is a valid manifest `publisherId`.
   - Signatures are 128 lowercase hex characters.
   - Every signed statement is serialized canonically: compact JSON, with fields in the fixed order of the Rust type and no optional fields. It is prefixed by a domain-separation line (`zeron.release-signature.v1\n`, `zeron.key-rotation.v1\n`, `zeron.key-revocation.v1\n`, `zeron.catalog.v1\n`), so a signature of one kind can never be replayed as another.
   - Verification re-serializes the parsed statement and checks the signature over those bytes. Parsing refuses unknown fields.
2. **Publisher identity.**
   - A publisher's identity is the first key it created, its *root key*. It is written into every release's `course.json` as `publisher.publisherId`.
   - A rotation retires the current key and signs a *rotation statement* `{ previousKey, nextKey, rotatedAt }` with the retiring key.
   - The chain of rotations from the root to the current key travels with every release, so learners need only the root.
3. **Release signature.** A release package may contain `zeron-signature.json` at its root, beside `course.json`. It holds:
   - the statement `{ courseId, courseVersion, manifestSha256, publisherKey, signedAt }`, signed by the current key;
   - the rotation chain.

   The manifest digest commits to every resource's SHA-256, so the signature covers the whole package. The torrent covers the signature file, so the info hash commits to it. Packages without the file stay valid and are shown as unsigned.
4. **Key storage.**
   - The secret key is held only in the operating system's credential store, under service `app.zeron.publisher`, through `keyring-core`:
     - macOS Keychain (`apple-native-keyring-store`);
     - Windows Credential Manager (`windows-native-keyring-store`);
     - the Secret Service on Linux (`dbus-secret-service-keyring-store`, which uses the `libdbus` Zeron already links).
   - Public keys, the rotation chain, and revocations are not secret. They live in the publisher database.
   - The secret never crosses IPC, never enters SQLite, and is never logged. In memory it is zeroized on drop.
   - Linux needs a running Secret Service (GNOME Keyring or KWallet). Without one, signing reports that no system keyring is available. This is the only platform limitation, and it does not apply to Windows or macOS.
5. **Backup.**
   - The publisher can export a backup file `zeron-publisher-key.json`. The key is encrypted with XChaCha20-Poly1305 under a key derived from a passphrase with Argon2id (64 MiB, 3 passes, a random 16-byte salt, and a 24-byte nonce), with the public key as associated data.
   - Restoring checks that the decrypted secret produces the recorded public key before storing it.
   - The passphrase must be at least 12 characters.
   - Losing both the credential store entry and the backup means the identity cannot sign again. That is stated in the UI.
6. **Learner trust: trust on first use, by course.**
   - The first signed version of a course the Library verifies pins that course's root key (`publisherId`).
   - A later version must verify and chain to the pinned root. Otherwise the import is refused, so an impostor cannot replace a course. This applies to a different root, a broken chain, an unsigned package where a signature was pinned, or a revoked key.
   - A learner who really wants the other publisher's course removes the course first. Pins are removed with it (ADR 0015).
   - Unsigned courses that were never signed import as before and are labelled "Unsigned".
7. **Revocation.**
   - A *revocation statement* `{ revokedKey, revokedAt, reason }` is signed by the revoked key itself or by a key later in the same chain.
   - Revocations travel in the catalog. A release signed by a revoked key with `signedAt` at or after `revokedAt` is shown as "Signed with a revoked key" and is refused as an update.
8. **Catalog.**
   - A catalog is a JSON document `{ publisherId, title, entries, revocations, rotations, issuedAt }`, signed by the publisher's current key.
   - Each entry is `{ courseId, courseVersion, title, description, magnet, infoHash, manifestSha256 }`.
   - **Hosting:** the publisher exports the catalog to a file and puts it anywhere: a website, a USB stick, or a chat. No Zeron server is involved.
   - **Explicit publication confirmation:** a release is listed only after the publisher confirms it for the catalog. Confirmations are stored per release, and the export lists only confirmed releases that are not archived.
   - **Discovery:** the Library imports a catalog from a file or an HTTPS URL, verifies it, and pins the catalog's publisher on first use like a course. It can refresh followed HTTPS catalogs on request.
   - **Import:** each entry imports through the existing magnet import. A catalog entry is a pointer; the release's own signature is what the Library trusts.
   - Magnet and `.torrent` import never require a catalog.
9. **Crates.**
   - `course-signing` is pure: statements, canonical bytes, signing, verification, rotation chains, and catalog documents. It has no storage, OS, or IPC.
   - `publisher-identity` handles key generation, the credential stores, and backups. It has no Tauri.
   - `publisher-application` signs during release preparation and builds catalogs.
   - `learning-application` verifies and pins.

## Consequences

- Courses signed by a publisher can no longer be silently replaced by someone else's package with the same `courseId`.
- Older Zeron builds refuse signed torrents, because the extra file is not described by `course.json`. Publishing a signed release is therefore an explicit choice, on by default once an identity exists.
- Trust on first use cannot protect the very first import. The fingerprint shown on both sides lets a learner compare it out of band.
- Real credential stores are exercised by tests on all three platforms. On Linux these need a Secret Service, so CI runs them with `gnome-keyring` under `dbus-run-session`. Unit tests use an in-memory store.
- Key storage, signing, and revocation are security-review triggers (AGENTS.md); this ADR is that review's record, together with `docs/security/threat-model.md`.
