# ADR 0017: Paid courses, learner licenses, and sign-in with Google or GitHub

Status: accepted in direction (2026-10-16). The user chose:

- paid courses unlocked by a license;
- no Zeron accounts;
- sign-in with Google or GitHub to recover purchases on another machine;
- creator verification through a GitHub proof.

The open questions at the end block only the slices that name them (roadmap Phase 11).

## Context

A torrent cannot be gated: anyone with the magnet downloads the bytes from any seeder. A paywall can therefore only gate **decryption**.

Payment is a fact in someone else's ledger (a card processor), which no offline signature check can see. Something online has to observe the payment and sign an entitlement. ADR 0014 gave publishers an Ed25519 identity and gave the Library trust on first use, per course. It gave learners no identity at all.

The user does not want Zeron accounts, meaning no Zeron passwords, profiles, or signup. A learner who changes machines must still get their purchases back.

## Decision

1. **What a paid course is.**
   - A release may mark lessons as *paid*. Each paid resource is encrypted; free preview lessons, the thumbnail, and `course.json` stay in plaintext. Learners can always see the title, the outline, and the previews.
   - The manifest gains a course-level `access` object. Slice 2 (2026-10-16) shipped it with `model` only; the issuer URL and delegation are added with the issuer slices:
     - `model: "paid"`;
     - the issuer URL;
     - the issuer delegation (decision 5).
   - Each encrypted resource gains `encryption`:
     - `scheme: "zeron.chunked-xchacha20poly1305.v1"`;
     - `chunkSize`;
     - `plaintextLength`.
   - A resource's `sha256` is the hash of the ciphertext. Integrity checks, torrent verification, and seeding therefore work without the key: anyone can seed a paid course, and that helps the swarm.
   - This is a manifest schema version bump. Older builds refuse paid packages; free packages are unchanged.
2. **Encryption format.**
   - Each release has a random 256-bit *content key*. Each resource's file key is derived from it with HKDF-SHA256, using the resource ID as the info string.
   - Files are split into fixed chunks of 65,520 plaintext bytes, so each sealed chunk (with its 16-byte tag) is exactly 64 KiB and never straddles a torrent piece boundary ([spike results](../research/paid-courses-spike-results.md)). Each chunk is sealed with XChaCha20-Poly1305 under:
     - a nonce derived from the file key and the chunk index;
     - associated data of `(resource ID, chunk index, final-chunk flag)`.

     This prevents reordering, truncation, and splicing between files.
   - The media gateway maps a plaintext byte range to chunks. It waits for verified torrent bytes as it does today, decrypts in Rust, and serves plaintext over the existing authenticated loopback. Seeking and lesson switching keep working.
   - Keys never reach the WebView. The content indexer and the PDF/transcript readers use the same decrypting reader, and only once a license is present.
3. **Learner identity: a key, not an account.**
   - Each installation creates a *learner key* in the operating system's credential store, the same way ADR 0014 stores publisher keys (service `app.zeron.learner`). It is a pair:
     - X25519, to receive wrapped content keys;
     - Ed25519, to sign requests to the issuer.
   - The secret never crosses IPC, never enters SQLite, and is never logged.
   - Each machine has its own learner key by default. Moving machines re-issues licenses to the new key (decision 7) instead of copying the secret.
   - An optional *learner key backup* uses the same passphrase format as ADR 0014's publisher backup. It is for learners who do not want to depend on Google or GitHub.
   - As built in slice 3 (2026-10-16):
     - The keychain holds one random 32-byte seed (account `learner-key`). The two keys are derived from it with HKDF-SHA256, salt `zeron.learner-key.v1`, info `signing` for the Ed25519 key and `encryption` for the X25519 key.
     - The Library's database records only the public identity (`ed25519-…`, `x25519-…`, creation time). This lets it tell "no key yet" from "this keychain lost the key". A key the keychain holds without a record is recorded again.
     - Backups are `zeron.learner-key-backup` version 1, with the associated data `zeron.learner-key-backup.v1\n` plus the public identity. Restoring never replaces a different key already in the keychain.
4. **License.**
   - A license is a signed statement `zeron.license.v1\n`, written in ADR 0014's canonical encoding:

     ```text
     { courseId, learnerKey, keys: [{ courseVersion, wrappedContentKey }], issuedAt }
     ```

   - `wrappedContentKey` is the release's content key sealed to the learner's X25519 key with HPKE (RFC 9180; X25519, HKDF-SHA256, ChaCha20-Poly1305, base mode) through the `hpke` crate 0.14.1. `info` is `zeron.license.v1`, and the associated data is the course ID and version.
   - Licenses are bound to a learner key, never bearer tokens. A copied license file is useless without that machine's secret.
   - The Library verifies licenses offline. It stores them as learner data, outside course directories, so they survive cache removal and course removal (ADR 0015).
5. **Issuer delegation.**
   - The publisher's key stays on the publisher's desktop. The issuer is a server with its own Ed25519 key.
   - The publisher signs `zeron.issuer-delegation.v1\n { issuerKey, issuerUrl, validFrom }` with their current key, and the delegation travels in the manifest's `access` object.
   - The Library accepts a license only if all of these hold:
     - an issuer key signed it;
     - a delegation, verified against the course's pinned publisher chain, names that issuer key;
     - neither key is revoked. Revocations use ADR 0014's statements; a delegation is revoked by revoking the issuer key in the publisher's catalog.

   A compromised issuer can therefore hand out licenses for the courses it serves. It can never sign releases or take over a publisher's courses.

   As built in slice 4 (2026-10-17):
   - **The delegation travels in the license file, not in the manifest.** The file is `{ format: "zeron.license", version: 1, publisherId, rotations, delegation?, license }`. The Library verifies it entirely offline against the course's pinned publisher, so no manifest change is needed. The manifest still gains the issuer URL with the issuer slices, as a hint for where to ask.
   - **The delegation statement adds `delegatedBy`,** the publisher key that signed it.
   - **The license statement adds `issuerKey`,** like a release's `publisherKey`.
   - **A license may also be signed directly by a key of the publisher's own chain,** with no delegation. That is how a publisher licenses a buyer by hand (Publish → release details → "License a learner").
   - **Revoking a delegated issuer key** uses ADR 0014's revocation statement, accepted when any key of the publisher's verified chain signed it, because an issuer key is not in the chain.
   - **Licenses are checked again every time they are used,** so a revocation learned later locks the course again.
   - **Paid courses must be signed:** without a pinned publisher there is nothing to verify a license against. Publish refuses to prepare a draft with paid lessons unless releases are being signed.
6. **The license issuer.**
   - The issuer is a small HTTPS service, the first server component since the seed server was removed. It:
     - receives each paid release's content key, sealed to the issuer's HPKE key, when the publisher registers the release;
     - handles one payment provider's checkout webhook;
     - issues licenses.
   - It stores only:
     - `(provider, subject)` → purchased course IDs;
     - the learner public keys (devices) bound to that subject, with when each was added and when it was last used;
     - registered releases and their sealed content keys.

     It does not store email addresses, names, or provider access tokens.
   - The repository ships a reference issuer (`services/license-issuer`) that a publisher can run. Who hosts issuers by default is an open question.
   - Free courses, magnet import, and `.torrent` import never contact an issuer.
7. **Sign-in with Google or GitHub.**
   - The issuer, not the desktop app, is the OAuth/OIDC relying party, so no client secret ships in the app. The flow:
     1. The app listens on a random `127.0.0.1` port and opens the **system browser** (RFC 8252; never the WebView, which Google blocks) at the issuer's `/auth/start`. The request carries the provider, the loopback redirect, and a PKCE challenge.
     2. The issuer runs the provider's flow:
        - Google: OIDC, scope `openid` only, verifying the ID token;
        - GitHub: OAuth with no scopes, then `GET /user` for the numeric ID.
     3. The issuer redirects to the loopback with a one-time code.
     4. The app redeems the code with the PKCE verifier, plus the learner public key and a signature over the code made with the learner Ed25519 key.
   - The result binds this machine's key to the signed-in subject and returns every license for that subject.
   - Buying records the purchase against the signed-in subject and the current machine's key. On a new machine, signing in re-issues all of that subject's licenses to the new key.
   - Each subject has a device limit. Learners see their machines in the Library and can remove one to add another.
   - The provider's tokens never reach the desktop app.
   - A learner who skips sign-in can still buy. The purchase is bound to the key alone, and only the learner key backup can recover it.
8. **New course versions.** A license holder asks the issuer for keys to later versions with a request signed by their learner key. No sign-in is needed. Whether a purchase covers future versions is the publisher's per-course setting.
9. **Refunds and revocation.**
   - A refund stops the issuer from re-issuing that purchase and from handing out keys for future versions.
   - A key that was already delivered cannot be taken back offline. The UI and the publisher documentation say so.
10. **Creator verification: a GitHub proof.**
    - The publisher signs `zeron.identity-proof.v1\n { publisherId, provider: "github", userId, login, createdAt }` with their current key and posts it as a public gist.
    - The catalog (ADR 0014) carries the gist URL.
    - When the Library is online, it fetches the gist from `api.github.com` and checks:
      - that the gist's owner ID equals `userId`;
      - that the statement verifies against the course's pinned publisher chain.
    - The Library then shows "Published by @login, verified via GitHub, checked on (date)". A deleted gist or a mismatch turns that into "no longer verified".
    - No issuer is involved, and courses import the same way whether or not they have a proof.
11. **Crates and boundaries.**
    - `course-access` is pure: the chunk format, licenses, delegations, identity proofs, and HPKE wrapping. It has no I/O.
    - `learner-identity` handles learner keys, the credential stores, and backups. It has no Tauri.
    - `license-client` is an adapter: HTTPS to the issuer, the loopback OAuth listener, and the gist fetch.
    - `learning-application` owns unlock, re-issue, device, and verification use cases behind ports.
    - The gateway gains a decrypting reader.
    - `publisher-application` handles encrypting at packaging, the delegation, and release registration.
    - Tauri commands return path-free, secret-free DTOs.

## Consequences

- Zeron gains an optional online dependency, but only for paid courses. Offline learning of a course already unlocked keeps working forever, because licenses verify offline.
- Paid content deters casual sharing but cannot prevent a paying learner from re-sharing decrypted files. Any client-side scheme has this weakness.
- Each issuer holds data about its learners: provider subjects and device public keys. It needs a privacy notice, deletion on request, and a security review (AGENTS.md triggers: signing/key storage, remote servers, new telemetry). `docs/security/threat-model.md` gains the issuer, licenses, and the decrypting gateway.
- Learners depend on their Google or GitHub account for convenient recovery. Losing that account and having no learner key backup loses the purchases.
- Google requires OAuth client verification for production. Asking only for `openid` keeps that review light.

## Open questions

1. Who hosts issuers: each publisher runs the reference issuer, Zeron hosts one shared issuer, or both? This blocks the issuer slice's deployment notes, not its code.
2. Which payment provider comes first, and is it a merchant of record that handles VAT and sales tax (Paddle, Lemon Squeezy) or a plain processor (Stripe)? This blocks the payment slice.
3. What is the default device limit (suggested: 3), and may publishers change it?
4. Do purchases include future versions by default?
5. Should crypto-wallet payment and identity follow as a later, optional slice?
