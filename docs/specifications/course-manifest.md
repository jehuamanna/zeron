# Course manifest and release schemas

## Status

This document defines the proposed version 1 contract. The implementation phase must publish formal JSON Schema files and fixtures generated/tested against the Rust types. JSON is UTF-8, duplicate object keys are rejected, unknown fields are rejected within v1 objects, and manifests have a configurable hard size limit (proposed default: 4 MiB).

## `course.json`

```json
{
  "schemaVersion": 1,
  "courseId": "01J00000000000000000000000",
  "version": "1.0.0",
  "title": "Distributed Systems",
  "description": "A practical course.",
  "publisher": { "name": "Example Publisher", "publisherId": "pub_example" },
  "language": "en",
  "category": "software-engineering",
  "tags": ["distributed-systems"],
  "license": { "name": "CC-BY-4.0", "url": "https://creativecommons.org/licenses/by/4.0/" },
  "prerequisites": ["Networking fundamentals"],
  "references": [{ "label": "Course site", "url": "https://example.invalid/course" }],
  "thumbnailResourceId": "res_cover",
  "estimatedDurationSeconds": 7200,
  "modules": [
    {
      "id": "mod_intro",
      "title": "Introduction",
      "position": 0,
      "lessons": [
        {
          "id": "lesson_welcome",
          "title": "Welcome",
          "position": 0,
          "estimatedDurationSeconds": 600,
          "resources": [
            {
              "id": "res_video",
              "kind": "video",
              "path": "modules/introduction/welcome.mp4",
              "mediaType": "video/mp4",
              "byteLength": 123456,
              "sha256": "<lowercase-hex>",
              "durationSeconds": 600,
              "relations": [{ "kind": "transcript", "resourceId": "res_vtt" }]
            },
            {
              "id": "res_vtt",
              "kind": "transcript",
              "path": "modules/introduction/welcome.en.vtt",
              "mediaType": "text/vtt",
              "byteLength": 1234,
              "sha256": "<lowercase-hex>",
              "language": "en"
            }
          ]
        }
      ]
    }
  ]
}
```

Required top-level fields are `schemaVersion`, `courseId`, `version`, `title`, `publisher`, `language`, and `modules`. Descriptions, categories, tags, license, references, thumbnail, prerequisites, and estimated duration are optional. Module, lesson, and resource order is explicit and positions must be unique contiguous integers within their parent.

Allowed v1 resource kinds are `video`, `audio`, `pdf`, `markdown`, `text`, `subtitle`, `transcript`, `image`, `source_archive`, and `other`. Relationships reference resource IDs, never paths. Resource paths use `/`, are relative, Unicode-normalized, contain no empty, `.` or `..` segment, no drive/UNC prefix, no NUL, and resolve inside the package root without traversing symlinks. Case-fold collisions are rejected for cross-platform portability. `course.json` cannot appear as a resource.

All IDs must be unique in the logical course namespace and match a conservative ASCII token grammar. URLs permit only explicitly allowed schemes (`https` initially). Durations are finite non-negative numbers. Declared lengths and hashes are checked against packaged bytes and torrent inventory.

## Paid courses: schema version 2

A course with paid lessons is written as `"schemaVersion": 2` ([ADR 0017](../decisions/0017-paid-courses-and-licenses.md)); every other course stays version 1, so older readers keep importing free courses. Version 2 adds two optional objects and nothing else:

```json
{
  "schemaVersion": 2,
  "access": { "model": "paid" },
  "modules": [{ "lessons": [{ "resources": [{
    "byteLength": 123488,
    "sha256": "<lowercase-hex of the sealed bytes>",
    "encryption": {
      "scheme": "zeron.chunked-xchacha20poly1305.v1",
      "chunkSize": 65520,
      "plaintextLength": 123456
    }
  }] }] }]
}
```

- `access` is present exactly when at least one resource carries `encryption`. `model` is `paid`; later slices add the license issuer.
- `encryption.scheme` must be `zeron.chunked-xchacha20poly1305.v1` and `chunkSize` must be `65520`.
- `byteLength` and `sha256` describe the **sealed** package bytes, so import, torrent inventory, and seeding checks are unchanged and need no key. `byteLength` must equal the sealed length of `plaintextLength`: the plaintext plus 16 bytes per started 65,520-byte chunk (at least one chunk).
- `schemaVersion` must be the version the content requires: 2 with `access` or any `encryption`, otherwise 1.
- The generated contract is `crates/course-format/schemas/course-manifest-v2.schema.json`; the version 1 file is kept unchanged for older readers.

Sealing: each resource's key is HKDF-SHA256 (salt `zeron.content.v1`, info = the resource ID) of the release's 256-bit content key. Chunk `i` is XChaCha20-Poly1305 with nonce = `i` as 8 little-endian bytes followed by 16 zero bytes, and associated data `"zeron.chunk.v1\n" ‖ u32be(len(id)) ‖ id ‖ u64be(i) ‖ (1 if final chunk else 0)`. Sealed chunks are concatenated with no header; each full one is exactly 64 KiB.

## Release record

The publisher database owns a release record; it is not mixed into `course.json` because torrent identity is computed from the package containing that manifest:

```json
{
  "schemaVersion": 1,
  "releaseId": "01J11111111111111111111111",
  "courseId": "01J00000000000000000000000",
  "courseVersion": "1.0.0",
  "manifestSha256": "<lowercase-hex>",
  "torrent": {
    "metainfoVersion": "v1",
    "infoHash": "<40-lowercase-hex>",
    "torrentFile": "artifacts/course-1.0.0.torrent",
    "magnetUri": "magnet:?xt=urn:btih:...",
    "pieceLength": 1048576,
    "pieceCount": 42,
    "totalContentBytes": 42000000
  },
  "packageDirectory": "releases/01J11111111111111111111111/package",
  "publishedAt": "2026-10-15T00:00:00Z",
  "status": "published",
  "publisherKeyId": null
}
```

Paths in this record are application-private and never sent to peers. Seeding status is a separate mutable job record. A future signed catalog envelope references the release and info hash but does not alter the torrent metainfo.

## Compatibility

Readers accept schema versions 1 and 2 and reject any other with a clear error. Additive evolution requires a new schema version and explicit reader tests; v1's unknown-field rejection prevents silently misunderstood security-sensitive data. The course version follows SemVer syntax but publisher policy determines meaning. Manifest canonicalization for signing is not specified by ordinary JSON serialization and requires a future ADR.

