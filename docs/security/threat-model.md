# Threat model

## Assets and adversaries

Assets include learner notes/history, publisher source media, immutable releases, signing keys, filesystem contents, network privacy, and process availability. Adversaries include malicious torrent peers/trackers, hostile course publishers, tampered packages, local web origins probing loopback, compromised remote seed servers, and accidental operator actions.

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
| Unauthorized remote control | disabled by default; TLS; identity verification; scoped revocable credentials | authn/authz/replay tests |
| Destructive operator error | distinct remove-job/delete-files actions; explicit target preview and confirmation | UI/E2E tests |
| Privacy surprise from seeding | opt-in/onboarding disclosure; visible status/rates; pause controls | usability and settings tests |

## Desktop capability policy

Each Tauri app gets a minimal allowlist. Dialog selection does not grant the frontend general filesystem APIs. Shell and arbitrary URL-opening capabilities are absent by default. Admin-only publishing access is not included in learner capabilities. CSP is explicit; development relaxations cannot ship. The loopback origin and dynamic port strategy must be validated per platform before final CSP is fixed. The Learner refuses top-level navigation away from the bundled app (ADR 0010).

## Cryptography and authenticity

BitTorrent piece hashes provide transport/content integrity under the info hash but not publisher identity. SHA-1 remains part of v1 torrent identity and is not reused as an authenticity primitive. Manifest resources also carry SHA-256 digests. Future publisher signatures use a reviewed canonical envelope and platform key storage; private keys never enter course packages or frontend state.

## Data retention and logs

Logs exclude magnet query parameters where secrets could appear, gateway URLs/tokens, local absolute course paths by default, note contents, signing material, and remote credentials. Diagnostic export is previewable and opt-in. Learner activity remains local unless a later analytics feature obtains separate consent.

## Open security decisions

- Key storage and recovery on each desktop OS.
- Exact WebView CSP/media behavior for randomized loopback ports.
- Sandboxing strategy for PDF.js, media probing, and optional FFmpeg.
- Remote seed-server provisioning and certificate rotation.
- Canonical serialization, key rotation, and revocation for signed catalog releases.

