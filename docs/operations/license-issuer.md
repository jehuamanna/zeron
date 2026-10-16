# Running a license issuer

The license issuer (`services/license-issuer`, [ADR 0017](../decisions/0017-paid-courses-and-licenses.md)) hands out licenses for paid courses, so buyers do not wait for the publisher to make one by hand. It is a reference server: small, with one SQLite file, one key file, and no accounts.

Who should run issuers is still an open product question (ADR 0017 open question 1). This guide covers a publisher running their own.

## What it holds

- **`issuer-key.json`:** the issuer's secret seed. It signs licenses, and it opens the course content keys publishers register. It is created readable only by its owner. Back it up and keep it private: whoever holds it can issue licenses for every course registered here.
- **`issuer.sqlite3`:**
  - each publisher's delegation and key chain;
  - which publisher owns each course here;
  - registered releases, whose content keys are **sealed to the issuer key**, so the database alone opens nothing;
  - entitlements by learner key;
  - a log of issued licenses.

  It holds no names, email addresses, or payment details.

## Setting up

```bash
cargo build --release -p license-issuer
```

```bash
target/release/license-issuer init --data /var/lib/zeron-issuer
```

`init` prints the issuer key and encryption key. A publisher needs both to delegate to this issuer and to register releases with it. `info` prints them again.

```bash
target/release/license-issuer serve --data /var/lib/zeron-issuer --listen 127.0.0.1:8787
```

`serve` speaks plain HTTP on the address given. Put it behind a reverse proxy that terminates TLS, for example Caddy or nginx, on the `https://` address that publishers write into their delegation. Do not expose port 8787 directly.

## Entitling a buyer

Until payment handling arrives (roadmap Phase 11 slice 6), entitle a buyer by their learner ID, which they copy from Zeron's Settings → Learner key:

```bash
target/release/license-issuer grant --data /var/lib/zeron-issuer --course <courseId> --learner zeron-learner:v1:…
```

The buyer's Zeron can then ask the issuer for a license. That request is signed with their learner key and must be within 5 minutes of the server's clock, so keep the clock synchronized.

## The HTTP interface

| Request | Body | Answer |
|---|---|---|
| `GET /v1/issuer` | none | `{ issuerKey, encryptionKey }` |
| `PUT /v1/delegations` | `{ publisherId, rotations, delegation }`, the delegation signed by a key of the publisher's chain | `204`; `422 delegation_invalid` |
| `POST /v1/releases` | `{ publisherId, rotations, registration }`, signed by the publisher's current key, with the content key sealed to `encryptionKey` for that course and version | `201` added, `200` already registered; `403 delegation_missing` or `course_owned`; `409 version_registered`; `422 registration_invalid` or `content_key_invalid` |
| `POST /v1/licenses` | `{ statement: { courseId, learnerKey, encryptionKey, issuerKey, requestedAt }, signature }` | `200` license file; `401 request_invalid` or `request_expired`; `403 not_entitled`; `404 course_unknown`; `503 delegation_not_valid_yet` |

Errors are `{ code, message }`. Bodies are limited to 64 KiB, and requests time out after 30 s.

**Course ownership:** the first publisher to register a course becomes its owner on this issuer, and nobody else can register versions of it. A version, once registered, keeps its content key.

**Versions in a license:** a license lists every registered version of the course. Whether a purchase should cover only versions released before it is ADR 0017 open question 4; this issuer currently includes later ones.

## Limitations

- **Revocations:** the issuer does not learn a publisher's key revocations. Revoke compromised keys in the publisher's catalog; learners' Libraries then refuse licenses signed under them.
- **No removal:** there is no way to remove an entitlement or a course yet. That comes with refunds in slice 6.
- **No rate limiting.** Use the reverse proxy for it.
