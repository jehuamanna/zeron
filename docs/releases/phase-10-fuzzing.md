# Phase 10: fuzzing the parsers

Status: complete. Every parser of untrusted input has a libFuzzer target, run on nightly in CI, and a seeded mutation test that runs on stable in every gate.

## Delivered

- **`fuzz/`** is a `cargo-fuzz` project kept outside the workspace, with its own lockfile. Seven targets:
  - `manifest`: accepted `course.json` files are valid and round-trip.
  - `metainfo`: the bencode decoder and the `.torrent` readers never panic, and decoded values re-encode canonically.
  - `range`: accepted gateway `Range` values lie inside the resource.
  - `release_signature`: parsing and verifying `zeron-signature.json` never panics.
  - `catalog`: parsing and verifying catalogs never panics.
  - `transcript`: WebVTT and SubRip parse to ordered cues or are refused.
  - `configuration`: accepted publisher configuration files round-trip.
- **`.github/workflows/fuzz.yml`** runs every target on nightly: 30 s each on pushes to `main`, 10 minutes each weekly, or a chosen length on demand. Crash inputs are uploaded as artifacts.
- **`mutation-testing`** is a new dev-only crate: a deterministic xorshift generator and mutators (bit flips, overwrites, inserts, deletes, duplication, truncation, splicing, and values that often change parsing), bounded in length. A failure names the seed and the mutation number.
- **Stable robustness tests** run thousands of mutations from valid seeds in milliseconds:
  - `course-format`: accepted manifests validate and round-trip.
  - `torrent-metainfo`: a real built torrent and bencode seeds never panic `decode`, `decode_dict_with_spans`, `info_hash`, `file_table`, or `inspect_torrent`, and decoded values re-encode.
  - `media-gateway`: mutated `Range` values give ranges inside the resource for lengths 0, 1, 99, 1,000, and `u64::MAX`.
  - `course-signing`: a mutated release signature or catalog that still verifies has exactly the original statement, so no mutation forges a statement.
  - `learning-domain`: transcripts give ordered cues, and note anchors, note documents, and drawing scenes never panic.
  - `publisher-application`: accepted configuration files round-trip.
- **`media-gateway::parse_range_header`** is now public so it can be fuzzed. Behaviour is unchanged.

## Acceptance evidence

- All the robustness tests above pass in the gates.
- `cargo check` builds the fuzz targets on stable. The nightly fuzz run is recorded in `phase-10-complete.md`.
- All repository gates pass.

## Current limitations

- Fuzzing runs in CI only; there is no persistent corpus between runs yet.
- PDF.js and media files are parsed by the WebView and are not fuzzed by Zeron.
