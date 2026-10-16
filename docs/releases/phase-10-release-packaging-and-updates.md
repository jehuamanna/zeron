# Phase 10: release packaging, signed updates, and documentation

Status: complete, except for the steps that need your credentials (below). A version tag builds Zeron for Linux, macOS, and Windows into a draft GitHub release. Installed copies can check for an update when the user asks, and install it only if it is signed with Zeron's release key. User and operator documentation is written.

## Delivered

- **Signed updates:**
  - `tauri-plugin-updater` 2.14.0 is in, with `rustls-tls` only. Its `zip` feature is off: its `zip` dependency has only yanked versions, and none of Zeron's update formats (AppImage, macOS `.app.tar.gz`, NSIS) need it.
  - `tauri.conf.json` carries the updater's minisign public key and the GitHub "latest release" endpoint, and turns on `createUpdaterArtifacts`.
  - **Commands:** `check_for_update` asks the endpoint only when called. `install_update` downloads, verifies the signature, installs, and restarts.
  - Errors: `update_not_trusted` (signature), `update_unavailable` (platform), `update_failed` (network, without addresses), `no_update`.
  - **UI:** Settings → Updates says the release page is contacted only on request and that updates must be signed. It offers "Check for updates", then "Install <version> and restart".
- **Packaging** (`.github/workflows/release.yml`, on `v*` tags or by hand), through `tauri-action`:
  - **Linux:** an AppImage that bundles GStreamer, built on Ubuntu 22.04 for older glibc.
  - **macOS:** a universal `.app` and `.dmg` (minimum macOS 11, `tauri.macos.conf.json`).
  - **Windows:** an NSIS installer that downloads the WebView2 bootstrapper (`tauri.windows.conf.json`).
  - Updater signatures and `latest.json` go into a draft release. Optional Apple signing and notarization secrets are passed through when set.
- **Reproducible inputs:**
  - `rust-toolchain.toml` pins Rust 1.99.1 for the gates, CI, and releases; `fuzz/rust-toolchain.toml` uses nightly. CI now installs the pinned toolchain rather than whatever stable is current, which is how a newer clippy broke Windows earlier.
  - Builds use `--locked` and `--frozen-lockfile`, with `SOURCE_DATE_EPOCH` set from the tagged commit.
- **Documentation:**
  - [`docs/user-guide.md`](../user-guide.md): Library, publishers and trust, learning, Publish, signing, sharing, and settings.
  - [`docs/operations/releasing.md`](../operations/releasing.md): key custody and rotation, secrets, cutting a release, reproducibility, and the release checklist.
  - [`docs/testing/platform-matrix.md`](../testing/platform-matrix.md): what is verified where, and media formats per web engine.
  - The README now describes the product and how to run the audits.

## Acceptance evidence

- The app builds with the updater (`pnpm --filter @zeron/desktop tauri build --debug --no-bundle`).
- **Vitest:** no check until clicked; up-to-date; an available version with installation; the untrusted-update message; no accessibility violations.
- All repository gates pass.

## Needs you

1. **Release secrets.** Add `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from `~/.config/zeron-release/` (see `releasing.md`). Without them the release workflow cannot sign updater artifacts.
2. **Platform signing.** Apple Developer and Windows code-signing certificates are needed for builds that Gatekeeper and SmartScreen trust.
3. **The first release.** Push a `v0.1.0` tag (or run the workflow by hand), review the draft, and publish it. Zeron did not create a release, because publishing one is your decision.

## Current limitations

- No release has been built in CI yet, so the packaging workflow is unexercised until the first tag.
- Updates are pulled on request only; there is no background check or staged rollout.
- The updater key was generated on this machine. Keep an offline copy, or regenerate it before the first release.
