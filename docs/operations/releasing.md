# Releasing Zeron

This is for the maintainers who build and publish Zeron itself, not for course publishers.

## One-time setup

1. **The updater key.** The updater's public key is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). Its private key and password were generated on 2026-10-16 into `~/.config/zeron-release/`: `updater.key` and `updater.key.password`, both readable only by the owner. Keep an offline copy. Losing the key means installed copies can never be updated automatically; the only way out is a manual reinstall with a new key.
2. **GitHub secrets** (Settings → Secrets and variables → Actions):
   - `TAURI_SIGNING_PRIVATE_KEY`: the contents of `updater.key`;
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the contents of `updater.key.password`.
3. **Optional, for builds the operating systems trust:**
   - **macOS:** `APPLE_CERTIFICATE` (base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific), and `APPLE_TEAM_ID`, for signing and notarization.
   - **Windows:** Authenticode signing needs a code-signing certificate, configured under `bundle.windows` in `tauri.windows.conf.json`.
   - **Without these:** macOS Gatekeeper and Windows SmartScreen warn on first launch. The updater's own signature check still applies.

To rotate the updater key, run `pnpm --filter @zeron/desktop tauri signer generate -w <file>`, commit the new public key, and update both secrets. Copies installed with the old key cannot verify updates signed with the new one, so announce the change.

## Cutting a release

1. Make sure CI on `main` is green on all three platforms, and that the weekly fuzz run passed.
2. Bump `version` in `apps/desktop/src-tauri/tauri.conf.json` and `apps/desktop/package.json`, and add a release note under `docs/releases/`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
4. The `Release` workflow (`.github/workflows/release.yml`) builds:
   - **Linux:** an AppImage that bundles GStreamer, plus its signature;
   - **macOS:** a universal `.app` and `.dmg`, plus the signed `.app.tar.gz` for the updater;
   - **Windows:** an NSIS installer and its signature;
   - `latest.json`, which installed copies read when the user clicks "Check for updates".
5. Review the draft release. Install each build on its platform and check the release checklist below. Then publish the draft. Updates reach users only after the release is published.

## Reproducibility

- **Rust:** pinned in `rust-toolchain.toml`. Dependencies are locked (`Cargo.lock`, built with `--locked`).
- **JavaScript:** locked (`pnpm-lock.yaml`, installed with `--frozen-lockfile`) with pnpm pinned through `packageManager`.
- **Timestamps:** `SOURCE_DATE_EPOCH` is set from the tagged commit.
- **Bundled media framework:** the GStreamer plugins inside the Linux AppImage come from the runner's Ubuntu 22.04 packages, so they track Ubuntu's updates rather than being pinned bit for bit.

## Release checklist

- [ ] The app starts on Linux, macOS, and Windows, and Library and Publish open.
- [ ] Media playback works on each platform; see [docs/testing/platform-matrix.md](../testing/platform-matrix.md).
- [ ] A signed release published on one computer imports by magnet on another as "Verified publisher".
- [ ] Settings → Updates on the previous version finds and installs the new one.
- [ ] Backup and restore work.
- [ ] `cargo deny check` and `pnpm licenses:check` pass (CI runs both).
- [ ] The soak test passes: `ZERON_SOAK_SECONDS=300 cargo test -p media-gateway --test soak -- --ignored`.
