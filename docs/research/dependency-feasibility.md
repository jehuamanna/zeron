# Dependency feasibility: offline-first Tauri learning platform

Status: researched 2026-10-15 against primary project documentation and source. Versions named below are a snapshot, not an instruction to float dependencies; implementation should pin exact versions and commit lockfiles.

## Executive conclusion

The proposed stack is feasible for a production-oriented first release if its first torrent format is explicitly **BitTorrent v1**, the media gateway is treated as an application-owned security boundary, and streaming is built as a thin adapter over librqbit's existing `FileStream` rather than as a second torrent scheduler.

The important qualifications are:

- librqbit 9.0.1 already resolves magnets, enumerates/selects files, persists sessions, downloads/seeds, creates v1 torrents, and exposes a verified-piece-aware asynchronous stream. Its bundled HTTP handler supports ordinary single byte ranges and seeking.
- librqbit does **not** expose a stable public API for arbitrary per-piece priority weights, verified buffered-range inspection, configurable per-stream lookahead, or explicit cancellation/deprioritization of stale lookahead. Its current stream implementation internally prioritizes a fixed 32 MiB window from the current read position.
- librqbit's current torrent creator is suitable as a starting point, not as the final publisher implementation: it is v1-only, uses a fixed 2 MiB default, does not sort directory traversal explicitly, silently drops directory-walk errors, assumes UTF-8 paths with `unwrap`, and does not expose web seeds/comment/created-by/creation date despite the metainfo type containing some of those fields.
- Tauri 2 can host this architecture directly in the Rust process. A sidecar is unnecessary for librqbit and would add packaging and lifecycle failure modes. A loopback Axum server is compatible in principle, but CSP, origin behavior, authentication, and media request patterns must be tested on WebView2, WKWebView, and WebKitGTK before the interface is frozen.
- SQLite FTS5 is a good fit. Prefer one bundled SQLite implementation and assert FTS5 at startup/tests. `rusqlite` is the narrower choice for a local desktop database; run it on a dedicated blocking database executor rather than on Tokio workers.

## Capability matrix

| Requirement | Verified capability | Gap / risk | Narrow recommendation |
|---|---|---|---|
| Magnet metadata first | `Session::add_torrent` accepts magnets; the HTTP API exposes `resolve_magnet`; `list_only`/`AddTorrentOptions` can return the file list without starting a download. | A magnet's metainfo does not contain `course.json` bytes; the manifest's pieces still have to be selected and fetched. Metadata resolution and manifest acquisition are separate recoverable states. | Model `ResolvingMetadata -> FetchingManifest -> ValidatingManifest`. After metadata, select the manifest file, read it through `FileStream`, then parse only after the required pieces verify. |
| File selection | `AddTorrentOptions.only_files` and `Session::update_only_files` select file indices at add time and later. Boundary pieces may cover bytes from adjacent files, as expected for v1. | Selection is file-level, not a public arbitrary piece-priority API. | Keep a set of explicitly selected files. Let active `FileStream`s temporarily prioritize their own pieces; do not promise exact file isolation at shared pieces. |
| Progressive media | `ManagedTorrent::stream(file_id)` returns `FileStream: AsyncRead + AsyncSeek`. It waits when the current piece is absent, wakes only after piece completion, and reads from storage after the have-bit is set. Active streams feed upcoming pieces into the peer scheduler. | Lookahead is a private fixed 32 MiB constant. No public buffered-range or throughput-to-buffer policy API exists. | MVP: wrap `FileStream` in the local gateway and measure player-visible buffering. Upstream or carry a very small patch making lookahead configurable and exposing verified piece/range snapshots. Do not build a parallel downloader. |
| Seeking | `FileStream` implements `AsyncSeek`; changing position updates the stream state used by the priority iterator. The HTTP handler maps a valid `Range: bytes=start-end` to seek + bounded read and emits 206, `Content-Length`, and `Content-Range`. | Current handler accepts only one explicit start range; it does not implement suffix/multiple ranges. Its 416 response does not visibly add `Content-Range: bytes */length`. It has no explicit HEAD handler in this module. | Own a small gateway handler around `FileStream` with RFC-tested GET/HEAD, single-range support (including open/suffix forms if WebViews issue them), correct 416 metadata, cancellation/timeouts, and bounded concurrency. Multiple ranges can initially return 416/unsupported if platform tests show media does not require them. |
| Integrity before playback | The stream checks librqbit's have-piece bit and stays pending until completion; piece completion follows torrent hash verification before storage is marked available. | The public API does not directly return a complete set of verified byte intervals. | Treat `FileStream` as the integrity boundary for reads. Add a read-only verified-range snapshot only for UI/buffer telemetry; never bypass it with direct file reads for incomplete torrents. |
| Lifecycle / recovery | `SessionOptions` supports JSON persistence, fast resume, caller cancellation token, and runtime limits. `Session` exposes pause, unpause, delete/forget, update selection, `stop`, and statistics. | App DB state and librqbit session state can diverge across crash/restart. | Make the application DB authoritative for user intent, reconcile it with librqbit on startup, and call `Session::stop()` during graceful exit. Keep crash recovery idempotent. |
| Download/seeding | DHT, trackers, peer exchange, TCP/uTP and seeding are present in rqbit; session/listener/rate-limit controls are public. | NAT reachability and tracker/DHT behavior vary by network. | Treat incoming reachability as an optimization, show actual status, and test tracker-only, DHT-only, offline resume, and no-inbound-connectivity cases. |
| Torrent creation | `create_torrent` and `Session::create_and_serve_torrent` generate a v1 metainfo, bytes, SHA-1 info hash, and magnet, then can seed it. `CreateTorrentOptions` has name, tracker list, and piece length. | See publisher gaps below. v2/hybrid is not implemented in current librqbit; an upstream v2 proposal remains open. | Ship v1 only. Build a deterministic, validated packaging layer and contribute/maintain a narrowly scoped creator patch. Validate generated bytes by reparsing and hashing, then add/seed the exact artifact. |
| Licensing | librqbit declares Apache-2.0 in its crate manifest and repository license. | Transitive-license review is still required for distributed binaries and optional features. | Record an SBOM/license report in release CI; disable unused librqbit features. |

Primary sources: [librqbit crate API](https://docs.rs/librqbit/latest/librqbit/), [`AddTorrentOptions`](https://docs.rs/librqbit/latest/librqbit/struct.AddTorrentOptions.html), [`Session`](https://docs.rs/librqbit/latest/librqbit/struct.Session.html), [`SessionOptions`](https://docs.rs/librqbit/latest/librqbit/struct.SessionOptions.html), [stream implementation](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/torrent_state/streaming.rs), [HTTP stream handler](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/http_api/handlers/streaming.rs), [piece acquisition](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/torrent_state/live/mod.rs), [file selection/priority ordering](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/file_info.rs), [rqbit HTTP API overview](https://github.com/ikatson/rqbit#http-api), [license](https://github.com/ikatson/rqbit/blob/main/LICENSE).

## librqbit findings in detail

### Metadata and file selection

`AddTorrentOptions` exposes both `list_only` and `only_files: Option<Vec<usize>>`; the HTTP facade maps these directly, and the live session exposes `update_only_files`. This is sufficient for metadata-first navigation followed by manifest-only and lesson-only selection. The application must preserve the distinction between:

1. torrent metadata resolved,
2. manifest file selected,
3. all pieces overlapping the manifest verified,
4. manifest parsed and semantically validated.

The course tree must not be fabricated at stage 1. For a magnet, stage 1 gets metainfo through BEP 9; it does not get the content of `course.json`.

Relevant primary sources: [`AddTorrentOptions`](https://docs.rs/librqbit/latest/librqbit/struct.AddTorrentOptions.html), [HTTP API types mapping](https://docs.rs/librqbit/latest/src/librqbit/http_api_types.rs.html), [API facade including update selection and streaming](https://docs.rs/librqbit/latest/src/librqbit/api.rs.html).

### What “streaming” actually guarantees

The current source is stronger than a generic read-from-partial-file approach:

- `FileStream::poll_read` computes the torrent piece containing the current file position.
- If that piece is not in the have bitfield, it registers a waker and returns `Poll::Pending`.
- Piece completion wakes streams waiting on that exact piece.
- Only then does it pread bytes from the torrent storage.
- `AsyncSeek` updates the current stream position, which changes the upcoming priority window.
- The live peer scheduler consumes pieces yielded by active streams before ordinary selected-file work.

This is a sound primitive for “never serve unverified bytes.” It also provides natural backpressure: the Axum response body polls the reader and stops producing bytes while a piece is unavailable. Dropping the response drops the `FileStream`, which unregisters its stream state.

However, the scheduler is intentionally internal. The source currently defines a 32 MiB per-stream lookahead and interleaves active streams; it does not expose the product-level signals requested by the proposed `BufferController` (verified byte intervals, adjustable low/high watermarks, or stale-range cancellation). The rqbit TODO also lists file priority and start/end priority refinements, evidence that these are not stable completed public controls.

Primary sources: [stream implementation](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/torrent_state/streaming.rs), [peer piece acquisition](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/torrent_state/live/mod.rs), [rqbit TODO](https://github.com/ikatson/rqbit/blob/main/TODO.md).

### Recommended minimal streaming extension

Keep the adapter surface engine-neutral, but implement it using librqbit:

```text
open_file(torrent_id, file_id) -> async seekable verified reader
set_selected_files(torrent_id, set<file_id>)
verified_ranges(torrent_id, file_id) -> snapshot/runs
set_stream_lookahead(stream_id, bytes)          // small librqbit patch
subscribe_piece_completed(torrent_id)           // optional telemetry hook
```

Do **not** require a general `set_piece_priority(piece, weight)` for the first release. The existing stream position already supplies the essential seek priority. Only add an explicit priority API if cross-platform playback testing demonstrates that configurable lookahead alone cannot maintain the buffer.

### Built-in HTTP API versus application gateway

rqbit's optional `http-api` feature uses Axum and its stream route already emits MIME type, `Accept-Ranges`, 200/206, length and range headers, and a 64 KiB streaming body. It is useful reference code, but enabling the complete rqbit server inside the app would expose torrent-management endpoints that the media WebView does not need. Its supported global authentication is HTTP Basic, not the requested per-session opaque resource authorization.

Therefore mount only an application-owned media router on an ephemeral loopback port:

- bind an explicit loopback address (`127.0.0.1`; separately decide/test `::1`), never a wildcard;
- register opaque resource IDs server-side; never put filesystem paths in routes;
- require an unpredictable session secret (prefer an unlogged path segment or same-origin bootstrap mechanism; query tokens leak more easily through logs/history); use constant-time comparison;
- keep the secret and full URL out of tracing, analytics, errors, and persistent state;
- allow only GET and HEAD; reject all others;
- implement one validated byte range per request, exact 206/416 headers, and bounded response chunks;
- cap concurrent waiting requests per playback session, add idle/header/body timeouts, and propagate disconnect cancellation by dropping the stream;
- omit permissive CORS by default. If a WebView requires CORS, allow only the exact application origin observed on that platform and validate it in the integration suite;
- stop the listener before/with `Session::stop()`.

This wrapper reuses librqbit's verified reader and scheduler; it does not copy whole media files or create a second cache.

Primary sources: [rqbit HTTP handler](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/http_api/handlers/streaming.rs), [librqbit Cargo features (`http-api` is optional)](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/Cargo.toml), [rqbit README authentication/API](https://github.com/ikatson/rqbit#http-api).

## Publisher / torrent creation

### Verified current creator

The current `CreateTorrentOptions` contains only `name`, `trackers`, and `piece_length`. `create_torrent` walks a directory, hashes a continuous v1 piece stream with SHA-1, serializes metainfo, computes the v1 info hash, and provides `.torrent` bytes and a magnet. `Session::create_and_serve_torrent` can immediately add the result for seeding.

This satisfies a developer prototype but not all mandatory publishing requirements. Direct source inspection identifies these gaps:

- directory entries are not explicitly sorted before hashing/serialization, so deterministic output is not guaranteed;
- `WalkDir` errors are discarded with `filter_map(|e| e.ok())`;
- non-UTF-8 paths panic through `to_str().unwrap()`;
- default piece size is a TODO returning 2 MiB regardless of corpus size;
- one tracker tier is constructed from a flat tracker vector;
- web seeds, comment, created-by and creation time are not configurable;
- there is no progress/cancellation/report API for a long hashing job;
- creation is v1 only. The open BEP 52 issue is a design proposal, not shipped v2/hybrid support.

Primary sources: [creator source](https://github.com/ikatson/rqbit/blob/main/crates/librqbit/src/create_torrent_file.rs), [`Session::create_and_serve_torrent`](https://docs.rs/librqbit/latest/librqbit/struct.Session.html), [open BEP 52 implementation design](https://github.com/ikatson/rqbit/issues/546).

### Recommended narrow fallback

For the first release, keep librqbit as the engine and implement a **small v1 metainfo-builder extension**, ideally upstreamable, rather than adopting another download engine:

1. The course packager produces a normalized staging directory and an explicit, lexically sorted list of safe relative paths.
2. The builder consumes that list (not an implicit walk), rejects symlinks/special files unless a policy explicitly permits them, reports every I/O error, and accepts a cancellation/progress callback.
3. It exposes tracker tiers, optional web seeds, comment and created-by, while keeping the `info` dictionary immutable after hashing.
4. It uses a documented, deterministic piece-size policy and records it in the publishing report.
5. It serializes once, reparses with librqbit, verifies the info hash/file table/piece count, then starts seeding those exact bytes and package files.

If changing librqbit is undesirable, isolate the creator behind `TorrentMetainfoWriter` and evaluate a dedicated writer. `lava_torrent` has a v1 builder but its own documentation says it is not recommended for critical systems, so it is not the preferred fallback. Newer `rbit`/`bitors` builders expose richer v1/v2/hybrid metadata, but they should not be introduced without fixture interoperability, determinism, maintenance, panic/fuzz, and license audits. None of them should replace librqbit's runtime merely to write metainfo.

Primary sources: [lava_torrent warning and API](https://docs.rs/lava_torrent/latest/lava_torrent/), [rbit builder surface](https://docs.rs/rbit/latest/rbit/metainfo/struct.TorrentBuilder.html), [bitors builder](https://docs.rs/bitors/latest/bitors/torrent/builder/).

## Tauri 2 integration

### In-process Rust is the default

Tauri commands are an appropriate control plane for library/course/download operations, but large media bytes should not traverse JSON IPC. The loopback gateway lets the native WebView's media stack perform HTTP range reads while Rust owns authorization and torrent state.

Tauri uses different system WebViews by platform: WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux. Consequently “works in Chromium” is not sufficient evidence for request patterns or codec support. The release gate needs packaged-app tests on all three, recording GET/HEAD/range behavior, seeks, cancellation on source change/window close, and origin headers.

Primary sources: [Tauri command model](https://v2.tauri.app/develop/calling-rust/), [Tauri process/WebView model](https://v2.tauri.app/concept/process-model/).

### CSP and loopback

Tauri applies CSP only when configured and automatically injects nonces/hashes for bundled assets. Configure an explicit CSP; permit the exact loopback media origin in `media-src` and, if PDF.js fetches from the gateway, in `connect-src`. Because the gateway uses a random port, confirm the chosen CSP host/port expression in each packaged WebView. Do not solve a CSP failure by broadly allowing `http:` or `*`.

A token in an HTTP header is awkward for a native `<video src>` request because application JavaScript cannot attach arbitrary headers to the media element's internal subrequests. Tokenized opaque URLs are therefore the practical baseline; keep them ephemeral and unlogged. This behavior must be validated rather than assumed, especially around redirects and subsequent range requests.

Primary source: [Tauri CSP guide](https://v2.tauri.app/security/csp/).

### Deep links and updates

The Tauri deep-link plugin supports desktop custom schemes. On Windows and Linux, a new process receives links as command-line arguments; the documented single-instance integration should be registered first. macOS schemes must be statically configured. Tauri explicitly warns that users can construct fake deep links, so `magnet:` or course-import links are untrusted input: length-limit, parse, confirm consequential actions, and never accept local paths or commands embedded in them.

The updater is feasible but orthogonal to offline operation. Tauri requires update signatures and production endpoints use TLS unless insecure transport is deliberately enabled. Keep updater signing keys outside the repository; losing the private key prevents publishing updates accepted by existing installations.

Primary sources: [Tauri deep linking](https://v2.tauri.app/plugin/deep-linking/), [Tauri updater](https://v2.tauri.app/plugin/updater/).

### Sidecars

Tauri can bundle target-triple-specific external binaries through `externalBin`, but librqbit is a Rust library and should be linked in-process. Reserve a sidecar for an optional media inspection/optimization tool such as FFmpeg/ffprobe only after its redistribution license, platform builds, sandbox surface, process supervision and cancellation are designed. Media optimization must remain an explicit publisher action.

Primary source: [Tauri sidecar packaging](https://v2.tauri.app/develop/sidecar/).

## SQLite and FTS5

### Recommendation

Use `rusqlite` with bundled SQLite, plus a small dedicated database worker/connection abstraction. This application is local and mostly single-writer; an async SQL connection pool adds limited value while SQLite calls remain blocking at the C boundary. Keep transactions and migrations in the storage crate and expose async application methods by sending jobs to the DB worker (or carefully using `spawn_blocking`).

`rusqlite` is actively developed, MIT-licensed, and recommends its `bundled` feature for applications that control their own database because it avoids old or absent system SQLite. Its broader `bundled-full` feature exists, but enable only the minimal features actually used. At startup and in CI, execute `SELECT sqlite_compileoption_used('ENABLE_FTS5')` and a tiny `CREATE VIRTUAL TABLE ... USING fts5(...)` smoke test rather than assuming a build flag.

SQLite documents FTS5 as a virtual table module supporting phrases, prefixes, NEAR/boolean queries, ranking, snippets/highlighting, external-content tables, and several tokenizers. Use ordinary normalized tables as the source of truth and an external-content/contentless FTS design only after testing trigger/rebuild correctness. Treat user queries as FTS syntax input, bind them as parameters, catch syntax errors, and never interpolate SQL.

Primary sources: [rusqlite project README](https://github.com/rusqlite/rusqlite), [rusqlite feature manifest](https://github.com/rusqlite/rusqlite/blob/master/Cargo.toml), [SQLite FTS5 documentation](https://www.sqlite.org/fts5.html).

### Alternative

SQLx supports bundled SQLite by default, async pools, and embedded migrations. It is reasonable if the repository strongly values a uniform async repository layer or compile-time query tooling. If selected, pin the shared `libsqlite3-sys` version and ensure no crate links a second incompatible SQLite native library. SQLx documents that bundled SQLite is its recommended static-link path and that migration-directory changes need an explicit stable-Rust build-script watch setup.

Primary sources: [SQLx SQLite driver](https://docs.rs/sqlx/latest/sqlx/sqlite/), [SQLx migrations](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html).

## PDF and media constraints

PDF.js supports URL loading via Fetch/XHR under same-origin/CORS rules and exposes range, streaming, and auto-fetch controls; its default range chunk size is 64 KiB. The same authenticated loopback resource registry can serve PDFs, but CORS/CSP must be exact and the gateway must tolerate PDF.js's request cadence. If strict on-demand behavior matters, test `disableAutoFetch` together with the documented requirement to disable streaming; do not assume one option alone prevents prefetch.

Primary source: [PDF.js `getDocument` API](https://mozilla.github.io/pdf.js/api/draft/module-pdfjsLib.html).

Native HTML media codec availability follows the installed system WebView and OS media stack, not merely the file extension. Adopt a conservative recommended publishing profile (MP4 container, H.264/AVC video, AAC audio; fragmented/progressive-friendly metadata with `moov` before media data) but classify it as “recommended and verified on the support matrix,” not universally guaranteed. The packager should inspect container/codec/duration/fast-start placement and warn; optimization/transcoding must be an explicit operation that creates new release bytes and therefore a new torrent identity.

Primary sources: [Tauri WebView engines](https://v2.tauri.app/concept/process-model/), [MDN media codec guide](https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Video_codecs).

## Required feasibility spikes before architecture sign-off

1. **Cross-platform streaming fixture:** a multi-file v1 torrent with shared boundary pieces and a fast-start MP4. On all packaged WebViews, measure time-to-first-frame, exact request headers, forward/backward seeks, cancellation, offline replay, and two simultaneous readers.
2. **Manifest-first fixture:** magnet resolution followed by selection and streaming of a small `course.json`, proving the UI can render structure without selecting videos.
3. **Gateway conformance/security:** GET, HEAD, open/closed/suffix ranges, invalid/overflow ranges, 416 headers, disconnect cancellation, concurrency cap, token rejection, traversal attempts, shutdown and log redaction.
4. **Creator determinism/interoperability:** build twice from identical normalized input and compare bytes; parse with librqbit and at least two independent clients; mutate one byte and prove verification fails; exercise Unicode/non-UTF-8/case-collision/symlink/permission-error fixtures on supported filesystems.
5. **Recovery:** kill during metadata resolution, manifest download, media streaming, final piece write, torrent creation and seeding; restart and reconcile DB intent with librqbit persistence without losing learner data.
6. **SQLite/FTS:** bundled build on every target, migration upgrade/downgrade policy, crash/WAL recovery, FTS rebuild, Unicode tokenization, malformed MATCH input and backup/restore.

## Decision record recommended for the architecture phase

- Approve librqbit 9.x behind a project-owned `TorrentEngine` adapter, pinning the exact tested release/commit.
- Approve v1 multi-file torrents for release 1; defer v2/hybrid until librqbit support is shipped and interoperability-tested.
- Approve an application-owned loopback media/PDF gateway backed by `FileStream`; do not expose the full rqbit HTTP API.
- Budget one small librqbit patch for configurable stream lookahead and verified-range telemetry only if the streaming spike proves it necessary.
- Budget a narrow, deterministic v1 metainfo-builder extension for publisher requirements; do not change download engines for torrent creation.
- Approve bundled `rusqlite` + FTS5 on a dedicated DB executor unless a repository-wide decision specifically favors SQLx.
- Keep codec optimization optional and explicit; package validation warns and reports but never silently rewrites source media.

