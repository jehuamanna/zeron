# Phase 10: dependency and license audit

Status: complete. Rust and JavaScript dependencies are checked against an explicit license allow-list and the RustSec advisory database on every push. See `docs/legal/dependency-audit.md` for the policy and each reviewed exception.

## Delivered

- **`deny.toml`:**
  - the permissive license allow-list;
  - per-crate MPL-2.0 exceptions for Tauri's HTML tooling;
  - advisories denied unless reviewed, with the two reviewed exceptions recorded and their reasons;
  - no yanked crates, unknown sources, or wildcards;
  - the graph checked for Linux, macOS, and Windows targets.
- **`tools/licenses/check-js-licenses.mjs`** (`pnpm licenses:check`) applies the same policy to the production JavaScript dependencies, with a reviewed map for packages whose metadata names no license.
- **CI:** a new `audit` job runs `cargo-deny-action` and the JavaScript check.

## Findings and decisions

- **`ringbuf` 0.4.8** (RUSTSEC-2026-0293) comes through librqbit's uTP. The fix needs `ringbuf` 0.5, which librqbit-utp 0.7.0 does not accept. Zeron's use stores only `u8`, so the double free cannot be triggered. It is accepted with that reason and must be revisited on the next librqbit upgrade.
- **`proc-macro-error`** (unmaintained) is build-time only, through Tauri's Linux GTK bindings.
- **MPL-2.0 crates** come through Tauri and are unavoidable without leaving Tauri. MPL-2.0 is accepted for those crates only, not as a general allowance.

## Acceptance evidence

- `cargo deny check` reports advisories, bans, licenses, and sources all ok.
- `pnpm licenses:check` passes for all 398 production packages.
- All repository gates pass.

## Current limitations

- Approval question 5 (license and size policy) is still open. The allow-list is a proposal until the user confirms it.
- A license notice bundle for distribution is not generated yet. The release packaging slice generates it.
