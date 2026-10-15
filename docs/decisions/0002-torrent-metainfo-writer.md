# ADR 0002: Dedicated deterministic BitTorrent v1 metainfo writer

Status: accepted (2026-10-15)

## Context

Publishing needs v1 multi-file metainfo whose `info` dictionary and info hash are identical for identical staged packages. The [dependency feasibility report](../research/dependency-feasibility.md#publisher--torrent-creation) found librqbit 9's creator unsuitable as-is for publishing: unsorted directory walks, swallowed walk errors, panics on non-UTF-8 paths, a fixed 2 MiB piece size, a single tracker tier, and no progress, cancellation, or report. It recommends a narrow, deterministic builder that consumes an explicit, sorted file list, keeping librqbit as the runtime engine. The engine is not integrated yet (Phase 3), and AGENTS.md forbids engine capabilities without a cited API and an integration test.

## Decision

1. Add an engine-neutral crate, `torrent-metainfo`, that owns:
   - a canonical bencode encoder and a strict, bounded decoder;
   - a v1 multi-file builder;
   - an independent verifier.

   It depends only on `sha1` (`=0.11.0`, the same RustCrypto `digest` generation as the pinned `sha2`), `url` (for magnet percent-encoding), and `thiserror`. It contains no librqbit types.
2. **Input is an explicit file list.** Packaging derives it from the validated `course.json` (every resource path plus `course.json`) and checks that the staged directory contains exactly those regular files: no extras and no symbolic links. Files are ordered bytewise by package path.
3. **Deterministic piece length.** It is the smallest power of two, at least 16 KiB, giving at most 2,048 pieces, capped at 16 MiB. The chosen value is returned so the publish report can record it.
4. **The info dictionary contains only `files`, `name`, `piece length`, and `pieces`.** Optional top-level fields are emitted only when explicitly supplied: tracker tiers (`announce` plus `announce-list`), web seeds (`url-list`), `comment`, `created by`, and `creation date`. Defaults omit the creation date, so whole-file bytes are deterministic too.
5. **Verification is a separate code path.** It decodes the bytes strictly (sorted, unique keys; no leading zeros; bounded depth and size), requires the `info` value to re-encode byte-identically, recomputes the info hash from the raw `info` span, checks the file table against the expected list, and rehashes every piece by streaming the files again.
6. Interoperability with librqbit (parse plus add-for-seeding) becomes a Phase 3 integration test when the engine adapter lands. Until then, the strict decoder and the BEP 3 field rules are the compatibility evidence.

## Consequences

- Metainfo creation has no engine dependency and cannot drift with engine upgrades.
- The project owns a small bencode implementation; its decoder is bounded and fuzz-tested because torrent files are untrusted input elsewhere.
- v2 and hybrid torrents remain out of scope, matching the engine's v1-only support.
- If librqbit later rejects a field layout, the fix belongs here and must keep the determinism tests green.
