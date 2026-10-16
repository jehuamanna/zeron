# Phase 11, slice 5: reference license issuer

Status: complete. Run against real HTTP on loopback in tests, with no public network. `services/license-issuer` is the first server component since the seed server was removed. A publisher runs it to hand out licenses for paid courses, so buyers no longer wait for a license made by hand. The desktop app does not talk to it yet: Publish registration is slice 8 and the Library's request is slice 7. This slice is the server and its protocol.

Decision record: [ADR 0017](../decisions/0017-paid-courses-and-licenses.md) decisions 5, 6, and 8, with this slice's choices recorded there. Operator guide: [license-issuer.md](../operations/license-issuer.md).

## Delivered

- **`course-signing`:** the issuer protocol, all pure.
  - `DelegationSubmission` and `verify_delegation_submission`: signed by a key of the publisher's chain that was not revoked at `validFrom`, for this issuer, with an `https://` address.
  - `ReleaseRegistration` (`zeron.release-registration.v1`), `RegistrationSubmission`, `sign_release_registration`, and `verify_release_registration`: signed by the publisher's current, unrevoked key, for this issuer.
  - `LicenseRequest` (`zeron.license-request.v1`), `sign_license_request`, and `verify_license_request`: signed by the learner key, which binds the stated X25519 encryption key to it, for this issuer.
- **`services/license-issuer`** (crate `license-issuer`, library plus binary):
  - **Keys:** `issuer-key.json` holds one seed, created owner-only (0600 on Unix) and never replaced. HKDF-SHA256 derives the license-signing Ed25519 key and the X25519 key content keys are sealed to. The seed never enters the database.
  - **Storage:** `issuer.sqlite3` at schema 1 holds:
    - publishers, with their delegation and longest known chain;
    - course owners;
    - releases, with content keys sealed to the issuer;
    - entitlements by learner key;
    - a log of issued licenses.

    It holds no names, email addresses, or payment details.
  - **HTTP:** `GET /v1/issuer`, `PUT /v1/delegations`, `POST /v1/releases`, and `POST /v1/licenses`, with JSON `{ code, message }` errors. Bodies are limited to 64 KiB, requests time out after 30 s, and storage and crypto work runs off the async threads. Internal failures are logged for the operator and reported to clients only as `issuer_failed`.
  - **Rules:**
    - A registration's sealed content key must open under the issuer's key for that course and version.
    - The first registering publisher owns the course here, and a registered version keeps its content key.
    - A license request must be within ±5 minutes of the issuer's clock, for a course and learner key with an entitlement, while the delegation is valid.
    - The license lists every registered version, each content key re-sealed to the learner, signed by the issuer, and carries the publisher's chain and delegation.
  - **CLI:** `init`, `info`, `serve --listen` (plain HTTP for a TLS proxy), and `grant --course --learner <learner ID>`.
- **Docs:** the operator guide covers setup, TLS behind a proxy, backups, entitlements, the HTTP interface, and limitations. The threat model gains the issuer.

## Acceptance evidence

- **`course-signing`:**
  - **Unit tests:**
    - delegations verify only for their issuer and from the chain; strangers and plain `http` are refused;
    - registrations must come from the current, unrevoked key and be unaltered;
    - license requests prove the learner holds both keys, and swapped encryption keys, other issuers, and garbled keys are refused.
  - **Mutations:** 6,000 mutated registrations and license requests never verify as changed ones.
- **`license-issuer` over HTTP** (`tests/issuer.rs`, a real server on `127.0.0.1:0`, a controlled clock):
  - **Full flow:** issuer info, delegation, registration (then the same registration again), and a grant. The license request returns a license that verifies with the Library's own `verify_license` against the publisher as delegated, and opens to the exact content key with the learner's X25519 secret. No content key appears in the clear.
  - **Later versions:** after a later version is registered, the next request returns both versions.
  - **Publisher refusals:** registration before delegation (`delegation_missing`); a delegation for another issuer (`delegation_invalid`); a changed content key for a registered version (`version_registered`); a rival publisher's version of an owned course (`course_owned`); a content key sealed for another course (`content_key_invalid`); a tampered registration (`registration_invalid`).
  - **Learner refusals:** an unknown course; a learner without an entitlement; a delegation not yet valid; a stale request; a swapped encryption key; a request for another issuer. A valid request then succeeds.
  - **Bad bodies:** malformed JSON (`400`), an oversized body (`413`), and a wrong method (`405`).
- **`license-issuer` binary** (`tests/cli.rs`):
  - `init` creates keys once and refuses to replace them;
  - `info` repeats them;
  - `grant` refuses a bad learner ID, entitles a valid one, and says when it already was;
  - errors for a missing data directory and unknown commands;
  - `serve` answers `GET /v1/issuer` on the chosen loopback port.
- **Unit tests:** the key file is created once, owner-only, reloads the same keys, and refuses a malformed file. Storage keeps course ownership, keeps each version's key, and stores entitlements once.
- **Repository gates:** all pass (Rust 441 tests, Vitest 218). `cargo deny check licenses bans` passes; the only new dependency is `signal-hook-registry` (MIT/Apache-2.0, through `tokio`'s signal handling).

## Manual verification (not yet run)

1. `license-issuer init --data <dir>`, then `serve`, behind a local TLS proxy such as `caddy reverse-proxy --from localhost:8443 --to 127.0.0.1:8787`.
2. Until the desktop app talks to the issuer (slices 7 and 8), drive delegation, registration, and a license request with the requests from `tests/issuer.rs`.

## Current limitations

- **Not wired into the app yet:** Publish does not yet delegate or register releases (slice 8), and the Library does not yet request licenses (slice 7).
- **Entitlements by command only:** they are granted with `grant`; payments (slice 6) add a webhook, and refunds add removal.
- **Revocations are not consulted:** the issuer does not know publishers' key revocations, so learners' Libraries enforce them.
- **No per-course "future versions" setting:** every registered version is included.
- **Plain HTTP:** TLS, rate limiting, and access logs belong to the reverse proxy.
