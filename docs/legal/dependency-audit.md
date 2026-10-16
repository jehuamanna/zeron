# Dependency and license audit

Checked on 2026-10-16, and again by CI on every push (the `audit` job in `.github/workflows/ci.yml`).

## Policy

- **Rust** (`deny.toml`, checked with `cargo deny check`):
  - permissive licenses only: Apache-2.0 (also with the LLVM exception), MIT, MIT-0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, BSL-1.0, and 0BSD;
  - no yanked crates, no unknown registries or git sources, and no wildcard versions;
  - security advisories from RustSec are denied unless reviewed below.
- **JavaScript** (`pnpm licenses:check`): the production dependencies must use the same permissive licenses, plus Unlicense and BlueOak-1.0.0. For a dual license, one acceptable side suffices.

Copyleft licenses need a decision first; that is roadmap approval question 5. The exceptions below are file-level copyleft components that Zeron ships unmodified.

## Reviewed exceptions

| Item | Why it is accepted |
|---|---|
| MPL-2.0: `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext` | Required by Tauri's HTML tooling and directory helpers. MPL-2.0 applies per file. Zeron does not modify them, and their source is public on crates.io. |
| RUSTSEC-2024-0370, `proc-macro-error` unmaintained | Build-time proc-macro used by gtk-rs 0.18, which Tauri needs on Linux. Nothing of it runs in Zeron. |
| RUSTSEC-2026-0293, `ringbuf` < 0.5.2 double free when an element's `Drop` panics | librqbit-utp 0.7.0 stores only `u8`, whose `Drop` cannot panic, so the bug cannot trigger. Revisit when librqbit moves to `ringbuf` 0.5. |
| `khroma` (JavaScript) without a `license` field | Ships an MIT license file. It is pulled in through Excalidraw's Mermaid support. |
| `dompurify` (MPL-2.0 OR Apache-2.0) | Used under Apache-2.0. |

## Result

- `cargo deny check`: advisories ok, bans ok, licenses ok, sources ok.
- `pnpm licenses:check`: all 398 production JavaScript packages have allowed licenses.
