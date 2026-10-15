# Phase 2: deterministic BitTorrent v1 metainfo and independent verification

Status: complete as a backend pipeline (see ADR 0002). The Admin publish workspace that runs staging and torrent generation as one observable, cancellable job is the next slice.

## Delivered

- New engine-neutral crate `torrent-metainfo` (depends on `sha1 =0.11.0`, `url`, `thiserror`; no librqbit types):
  - **Canonical bencode encoder** and a **strict decoder**. The decoder rejects unsorted or duplicate keys, leading zeros, `-0`, out-of-range integers, truncated strings, trailing bytes, and nesting deeper than 64. `decode_dict_with_spans` returns the raw byte span of each top-level value so the info hash is computed over the bytes exactly as written.
  - **v1 multi-file builder** over an explicit file list:
    - Files are sorted bytewise by path. Paths must be safe and unique.
    - Each file must be a regular file of the declared length; symlinked files and symlinked intermediate directories are rejected.
    - Hashing streams across file boundaries with progress and cancellation.
    - The `info` dictionary contains only `files`, `name`, `piece length`, and `pieces`.
    - Optional tracker tiers (`announce` plus `announce-list`), web seeds (`url-list`), `comment`, `created by`, and `creation date` appear only when supplied. Tracker URLs must be http, https, or udp; web seeds http or https; the name must be a single safe segment.
    - The magnet link includes the display name, trackers, and web seeds, percent-encoded.
  - **Piece length policy:** the smallest power of two of at least 16 KiB giving at most 2,048 pieces, capped at 16 MiB.
  - **Independent verifier:** it decodes strictly, requires the `info` value to re-encode identically, recomputes the info hash from its raw span, checks name, piece length, piece count, and the full file table against the expected list, and rehashes every piece through a separate streaming reader that also detects short or long files.
- `publisher_application::generate_torrent(staging_root, attempt_id, distribution, cancellation, progress)`:
  - accepts only exact `stage_<32 hex>` attempt IDs that are real directories;
  - re-reads and validates the staged `course.json` (bounded to 4 MiB) and derives the file list from it;
  - requires the attempt tree to contain exactly those regular files, with no extras, symlinks, or special files;
  - names the torrent `<course_id>-<version>` with `created by: Zeron Publisher` and no creation date;
  - writes `<attempt>.torrent` atomically through `<attempt>.torrent.partial`, refuses to overwrite an existing torrent, and verifies the bytes read back from disk, removing the file if verification fails;
  - returns a path-free `TorrentReceipt` (info hash, magnet, piece length and count, file count, total bytes).
- `verify_staged_torrent` re-verifies an existing attempt torrent against its package.
- `discard_abandoned_staging` also removes interrupted `stage_<32 hex>.torrent.partial` files.
- `PackagingCancellation::is_cancelled` lets torrent hashing share the staging cancellation token.

## Acceptance evidence

- `torrent-metainfo` tests:
  - Bencode: canonical round-trip vectors; rejection of every non-canonical and malformed form above, plus excessive depth and non-dictionary roots; 5,000 mutated inputs decoded without a panic.
  - Piece length: the policy table at 0 B, 32 MiB, 32 MiB + 1, 4 GiB, and 1 PiB.
  - Determinism: building from differently ordered file lists gives byte-identical torrents with three pieces spanning file boundaries, exact top-level and `info` keys, and the expected encoded first file entry.
  - Verification: matching info hash and name; a flipped byte in the package fails piece verification, and restoring it passes again; a mismatched file table and corrupted torrent bytes fail.
  - Optional fields: trackers, web seeds, comment, and creation date leave the info hash unchanged and appear correctly in the magnet.
  - Rejections: declared-length mismatch, unsafe paths (`..`, empty segments, absolute, backslash, drive prefix, `.`), duplicates, an empty list, a symlinked file, a symlinked directory, invalid options. Cancellation works.
- `publisher-application` torrent tests:
  - Two separate staging attempts of one draft snapshot produce byte-identical torrents and the same info hash, which `verify_staged_torrent` confirms. Regenerating is refused.
  - Trackers change only the magnet.
  - An extra staged file blocks generation without leaving a torrent; a modified staged byte fails re-verification.
  - Traversal-style, malformed, partial, and unknown attempt IDs are rejected.
  - Cancellation leaves no torrent, and abandoned `.torrent.partial` files are discarded while the attempt is kept.
- All repository gates pass.

## Current limitations

- librqbit has not parsed or seeded these torrents yet. That interoperability test arrives with the Phase 3 engine adapter. Until then the strict decoder and BEP 3 field rules are the compatibility evidence.
- Tracker and web-seed configuration has no UI and no persisted publisher settings yet.
- No v2 or hybrid torrents.
- The Admin app cannot run staging and torrent generation yet; the publish workspace job (progress events, cancellation, startup cleanup, report) is the next slice.
- Completed attempts and their torrents are retained until release activation defines retention.
