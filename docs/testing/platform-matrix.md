# Platform matrix

What is verified on each platform, and how. "CI" means the `CI` workflow on GitHub-hosted runners for every push to `main`.

| Area | Linux (Ubuntu 24.04 CI; Arch with Hyprland locally) | macOS 15 (CI) | Windows Server 2025 (CI) |
|---|---|---|---|
| Rust build, clippy, all workspace tests | CI and local | CI | CI |
| Frontend tests and typecheck | CI and local | CI | CI |
| Real keychain: create, rotate, back up, restore, forget | Secret Service through GNOME Keyring (CI under `dbus-run-session`, and local) | macOS Keychain (CI) | Windows Credential Manager (CI) |
| Torrent import, seeding, and streaming tests (loopback engines) | CI and local | CI | CI |
| Media gateway soak and fault tests | CI and local | CI | CI |
| Fuzzing (nightly, libFuzzer) | CI | — | — |
| Release packaging | AppImage (`release.yml`, Ubuntu 22.04) | universal `.app` and `.dmg` | NSIS installer |
| In-app media playback in the real window | Verified locally with WebKitGTK and GStreamer (`phase-4-linux-in-app-playback.md`) | Manual check pending (WKWebView; H.264 and AAC are native) | Manual check pending (WebView2; H.264 and AAC are native) |
| Native file and save dialogs | Manual check | Manual check | Manual check |
| Router port mapping on a real router | Pending (`phase-3-port-mapper.md`) | Pending | Pending |

## Media formats

Zeron serves files byte for byte and never transcodes. Playback depends on the platform's web engine:

- **Linux:** WebKitGTK with GStreamer. The AppImage bundles `gstreamer1.0-plugins-base`, `-good`, and `-bad` and `gstreamer1.0-libav` for H.264, AAC, VP9, and Opus.
- **macOS:** WKWebView: H.264, HEVC, AAC, and MP3; WebM since macOS 11.3, depending on version.
- **Windows:** WebView2 (Chromium): H.264, AAC, VP9, Opus, and MP3.

The Builder's media inspection warns about formats that might not play everywhere (`phase-2` media warnings). H.264 with AAC in MP4 works on all three.
