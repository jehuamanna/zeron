# Phase 9 complete: publisher identity, signing, and catalogs

Status: complete. The user asked on 2026-10-16 for signing that works on Windows and macOS without limitation (ADR 0014). Phase 9 delivered these slices:

| Slice | Release note |
|---|---|
| Signature formats, keychain-held keys, backups, and three-platform CI | `phase-9-signing-foundation.md` |
| Publishing signed releases (Publish → Identity) | `phase-9-signed-releases.md` |
| The Library verifies publishers and pins each course to its first one (learner schema v13) | `phase-9-library-verification.md` |
| Signed catalogs, revocations, and discovery (publisher schema v6, learner schema v14) | `phase-9-catalogs.md` |

## Windows and macOS

The real-keychain test creates, rotates, backs up, restores, and forgets signing keys in each platform's own credential store. CI runs it on every push:

| Platform | Credential store | Result |
|---|---|---|
| macOS 15 | Keychain (`apple-native-keyring-store`) | Passed in CI (run 38883930262, the "Real keychain" step with `ZERON_KEYCHAIN_TESTS=1`) |
| Windows Server 2025 | Credential Manager (`windows-native-keyring-store`) | Passed in CI (run 38891771033, the "Real keychain" step) |
| Ubuntu 24.04 | Secret Service through GNOME Keyring | Passed in CI on every run since the slice landed |
| Arch Linux (local) | GNOME Keyring | Passed locally |

The signing, verification, and catalog code is pure Rust (`ed25519-dalek`, `argon2`, `chacha20poly1305`) and behaves the same on every platform. The full workspace test suite, including the signed-import tests over real loopback torrents, runs on all three in CI.

## Fix found while testing

The first catalog test found a way around revocations: a thief with a retired key could sign releases without the later rotations, and the successor key's revocation then could not be checked. Revocations are now kept with the rotation chain they arrived with and checked against it (`RevocationRecord`).

## Current limitations

- Trust on first use: the first version of a course a learner imports is trusted as it comes. Fingerprints let people check it out of band.
- Older Zeron builds refuse signed torrents (ADR 0014).
- The publisher CLI publishes unsigned releases only.
