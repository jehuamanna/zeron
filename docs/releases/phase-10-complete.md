# Phase 10 complete: hardening and release

Status: complete, except for the steps that need the maintainer's credentials, hardware, or judgement, listed at the end. Phase 10 delivered these slices:

| Item from the roadmap | Delivered by |
|---|---|
| Fuzzing | `phase-10-fuzzing.md`: seven libFuzzer targets on nightly CI, plus seeded robustness tests in every gate |
| Threat-model review | The "Review of 2026-10-16" in `docs/security/threat-model.md`; ADR 0014 for signing and key storage |
| Dependency and license audit | `phase-10-dependency-audit.md`: `deny.toml`, `pnpm licenses:check`, and the CI `audit` job |
| Cross-platform media matrix | `docs/testing/platform-matrix.md`, with CI on Ubuntu 24.04, macOS 15, and Windows Server 2025 |
| Performance, soak, and fault tests | `phase-10-soak-and-fault-tests.md` |
| Accessibility | `phase-10-accessibility.md`: axe-core checks of the main screens |
| Updater signing and reproducible CI packaging | `phase-10-release-packaging-and-updates.md` |
| Backup and restore | `phase-10-backup-and-restore.md` |
| Operator and user documentation | `docs/user-guide.md`, `docs/operations/releasing.md` |

## Cross-platform fixes found by the new CI

Running every test on macOS and Windows for the first time found these:

- **Windows:**
  - A clippy release added `chunks_exact_to_as_chunks`, which fired in Windows-only path decoding. Rust is now pinned in `rust-toolchain.toml`, so lints change only on purpose.
  - Tauri needs `icons/icon.ico` to build there; macOS bundles need `icon.icns`.
  - Several test helpers used only by Unix-only tests are now built only on Unix.
  - The old-app data-import fixture kept a database handle open on purpose, and Windows cannot delete files someone holds. The fixture now copies the database and its write-ahead log while the writer is open. The import itself was correct.
  - Four other tests deleted their databases while still holding them open; they now close them first.
- **macOS:**
  - Temporary files sit behind `/var` → `/private/var`, so two tests now compare canonical paths.
  - On a fast loopback, the two-seeder stream test could finish before the learner reached the second seeder; the seeders are now capped.

## Needs the maintainer

1. **Release secrets:** add `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (`docs/operations/releasing.md`).
2. **Platform signing:** Apple Developer and Windows code-signing certificates.
3. **The first release:** push a `v0.1.0` tag, review the draft release, and publish it.
4. **Manual checks:**
   - media playback in the real window on macOS and Windows;
   - screen readers and contrast;
   - native dialogs;
   - a router that answers PCP, NAT-PMP, or UPnP.
5. **License and size policy** (roadmap approval question 5). The allow-list in `deny.toml` is a proposal until confirmed.
