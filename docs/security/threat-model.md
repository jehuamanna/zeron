# Threat model

## Assets and adversaries

Assets include learner notes/history, publisher source media, immutable releases, signing keys, filesystem contents, network privacy, and process availability. Adversaries include malicious torrent peers/trackers, hostile course publishers, tampered packages, local web origins probing loopback, and accidental operator actions.

## Principal threats and controls

| Threat | Primary controls | Verification |
|---|---|---|
| Path traversal / absolute paths | strict lexical path grammar; canonical containment check; no HTTP paths | malicious manifest fixtures on all OS path styles |
| Symlink escape / TOCTOU | reject symlinks initially; open under controlled roots; restat before packaging | swap/symlink race tests |
| Loopback gateway file disclosure | opaque registry IDs; per-session secret; no filesystem parameter; loopback bind | traversal/fuzz/auth integration tests |
| Cross-origin probing / token leak | no permissive CORS; restrictive CSP/connect/media sources; token redaction; short lifetime | hostile-origin browser test |
| Unverified media served | adapter exposes only verified ranges; fail closed on uncertainty | corrupt-piece and partial-read tests |
| Resource exhaustion | manifest/file-count/size limits; bounded queues/chunks/connections; timeouts; disk reservation checks | slowloris, huge-range, torrent bomb tests |
| Malformed metainfo/manifest/media/PDF | maintained parsers, input limits, process isolation for optional probes | fuzz corpora and regression fixtures |
| Arbitrary command execution | no shell interpolation; fixed executable; structured argv; external tools optional | hostile filename tests |
| Draft/release mutation | staging + verification + atomic activation; release read-only policy | crash/fault injection per stage |
| Secret exposure | backend-only key store; zero/redact where practical; no frontend DTO/logging | log and IPC contract audit |
| Impostor course update (same `courseId`, another publisher) | Ed25519 release signatures; trust on first use per course; refuse a different or missing signature once pinned (ADR 0014) | folder and loopback-torrent impostor tests |
| Stolen or retired signing key | key rotation chains; revocations carried by signed catalogs and checked against the catalog's chain; catalogs never roll back | stolen-retired-key and revocation tests |
| Signing key theft from disk | secret only in the OS credential store; passphrase backups with Argon2id and XChaCha20-Poly1305; no secret over IPC or in logs | real-keychain tests on Linux, macOS, and Windows CI; IPC DTO tests |
| Destructive operator error | distinct remove-job/delete-files actions; explicit target preview and confirmation | UI/E2E tests |
| Privacy surprise from seeding | opt-in/onboarding disclosure; visible status/rates; pause controls | usability and settings tests |

## Desktop capability policy

Each Tauri app gets a minimal allowlist. Dialog selection does not grant the frontend general filesystem APIs. Shell and arbitrary URL-opening capabilities are absent by default. Admin-only publishing access is not included in learner capabilities. CSP is explicit; development relaxations cannot ship. The loopback origin and dynamic port strategy must be validated per platform before final CSP is fixed. The Learner refuses top-level navigation away from the bundled app (ADR 0010).

## Cryptography and authenticity

BitTorrent piece hashes provide transport/content integrity under the info hash but not publisher identity. SHA-1 remains part of v1 torrent identity and is not reused as an authenticity primitive. Manifest resources also carry SHA-256 digests. Publisher signatures (ADR 0014) sign a domain-separated canonical statement over the manifest's SHA-256 with Ed25519; secret keys live only in the OS credential store and never enter course packages, IPC, or frontend state. The first signed version of a course pins its publisher (trust on first use); fingerprints let people confirm the first import out of band.

## Data retention and logs

Logs exclude magnet query parameters where secrets could appear, gateway URLs/tokens, local absolute course paths by default, note contents, signing material, and remote credentials. Diagnostic export is previewable and opt-in. Learner activity remains local unless a later analytics feature obtains separate consent.

## Open security decisions

- Exact WebView CSP/media behavior for randomized loopback ports.
- Sandboxing strategy for PDF.js, media probing, and optional FFmpeg.
- Trust on first use cannot protect a learner's very first import of a course; a pre-shared fingerprint or a future publisher directory would.


## Review of 2026-10-16

The Phase 7, 9, and 10 work added new inputs and outputs. Each is listed with its controls and the tests behind them.

| Surface | Threat | Controls | Tests |
|---|---|---|---|
| Catalog files and HTTPS catalog addresses | A forged or stale catalog, a server sending huge or slow bodies, credentials in addresses | Signature and chain verified; a catalog older than the one kept is refused; plain `https://` only, no user information, at most 2 KiB address, 15 s timeout, three redirects, 1 MiB body; entries are pointers, and each release still needs its own signature | `catalogs_list_releases_carry_revocations_and_never_go_back`, `only_plain_https_addresses_are_followed`, catalog robustness and fuzz tests |
| Key revocations | A stolen retired key signing releases without the later rotations | Revocations are kept with the chain they arrived with and checked against it; rotations made after a revocation break the chain | `a_stolen_retired_key_cannot_hide_its_revocation_by_leaving_out_the_chain` |
| Signing key at rest | Theft from disk, exposure over IPC or in logs | OS credential store only; backups use Argon2id (64 MiB, 3 passes) with XChaCha20-Poly1305, the identity bound as associated data, and the restore cost capped; DTOs carry public keys only | identity command tests, real-keychain CI tests on all three platforms |
| Forged, altered, or shared licenses (ADR 0017) | A license made by someone other than the course's publisher, edited to cover other versions, copied to another learner, or signed with a stolen key | Verified offline against the course's pinned publisher chain; the issuer must be a chain key or delegated by one; bound to one learner signing key, with content keys sealed by HPKE to that learner's X25519 key and bound to the course and version; re-verified on every use against current revocations; paid courses must be signed; files bounded to 128 KiB and fuzzed | `course-signing` license unit, mutation, and fuzz tests; `paid_courses` end-to-end tests |
| License issuer (ADR 0017) | A stolen database, forged registrations or delegations, another publisher claiming a course, a learner taking another's license, replayed or oversized requests | Content keys stored sealed to the issuer key, which lives in a separate owner-only file; delegations and registrations verified against the publisher's chain, and registrations only by its current key; the first registering publisher owns a course, and versions keep their key; license requests signed by the learner key, which binds the encryption key, within ±5 minutes; licenses sealed per learner; 64 KiB bodies and a 30 s timeout; plain HTTP only behind a TLS proxy | `license-issuer` HTTP and CLI tests, `course-signing` request unit and mutation tests |
| Learner key at rest (ADR 0017) | Theft from disk, exposure over IPC or in logs, a restore silently replacing the key licenses are bound to | One seed in the OS credential store (service `app.zeron.learner`), keys derived by HKDF; the same passphrase envelope as publisher backups, with its own format name and the public identity as associated data; restore refuses to replace a different key; DTOs carry public keys and the fingerprint only | `learner-identity` unit and real-keychain tests on all three platforms, `learner_key` command tests |
| Backup files | A crafted backup that corrupts data or blocks startup | Strict format, per-database SHA-256, `quick_check`, schema no newer than this build, nothing after the last database; restore staged and switched in at startup with the old data kept aside | `data-backup` tests |
| Publisher configuration files | Hostile trackers or settings | The same validators as the settings screens; strict JSON; 256 KiB bound; reviewed before applying | configuration unit, robustness, and fuzz tests |
| Distribution activity log | Paths, addresses, or private tracker passkeys stored or exported | `redact()` before storage; bounded to 2,000 events | redaction unit tests and the activity command test |
| Updater | A malicious update | Minisign signature against the public key compiled into the app; checks only on request; the private key is outside the repository | update command error mapping, UI test |
| Annotation moves | Overwriting a learner's drawing or progress | Moves only where the target has none; note revisions increase so open editors reload | storage and application migration tests |

Still open, in addition to the decisions listed above:
- The first import of a course is trusted as it comes (trust on first use).
- macOS and Windows builds are not yet signed by their platform vendors.
