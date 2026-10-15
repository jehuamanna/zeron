# Phase 4: in-app playback on Linux

Status: complete. Torrent lessons play inside the Learner window on Linux, with seeking and lesson switching, through an HTTPS loopback gateway (ADR 0005).

## Problem

After the streaming slice, the Learner's WebKitGTK `<video>` failed with error 4 in release builds. Installing H.264/AAC decoders (`gstreamer1.0-libav`) was not enough. WebKit's GStreamer log and a loopback listener showed two independent blockers:

- WebKit refuses plain-HTTP media on the secure `tauri://localhost` page and never connects, even to `127.0.0.1`.
- WebKit's media player accepts only listed protocols, and GStreamer has no source for an app-defined scheme. A `zeron-media://` attempt was dropped for that reason.

The same blocker applied to local-folder courses: release builds on Linux could not play them either. Earlier checks ran in dev mode, where the page itself is `http://localhost`.

## Delivered

- **`media-gateway`:**
  - `MediaGateway::start_tls` serves HTTPS on an ephemeral `127.0.0.1` port with a per-launch self-signed certificate (`rcgen`, `rustls` with `aws-lc-rs`, HTTP/1.1 over hyper, 10 s TLS handshake timeout).
  - `certificate_pem` exposes only the public certificate.
  - Registered URLs use `https://` when TLS is on. `start` (plain HTTP) is unchanged.
- **Learner:**
  - On Linux the gateway starts with TLS, and the main webview's context trusts exactly that certificate for `127.0.0.1` (`allow_tls_certificate_for_host`, `webkit2gtk` `v2_6`).
  - Other platforms keep plain HTTP.
  - The CSP adds `https://127.0.0.1:*` to `media-src`.

## Acceptance evidence

- **Gateway test:** a TLS client that trusts only the per-launch certificate gets `206` with `content-range: bytes 2-5/10` and body `2345`. A client without it is refused during the handshake, and a plain gateway has no certificate.
- **Real Learner window** (release build, WebDriver), with a new fixture: the user's 13-lesson, 2.3 GB course of fast-start H.264 1080p + AAC MP4s, published by Zeron in 20 s. It was seeded by the Admin (ready after 8 s) and imported from its magnet. The source folder was only read, and nothing from it is committed.
  - **"Design Amazon" (47 min, 0 % downloaded):** decoded at 1920×1080 and reached 5.3 s of playback 7.8 s after Play was clicked.
  - **Seek:** a seek to 70 % (32:49) was playing again after 3.7 s, with buffered ranges 0–15:22 and 27:25–39:10.
  - **Lesson switch:** "Design Netflix" played from 0 % after 9.6 s, and the previous lesson's capability was revoked.
  - **Fresh launches:** "Design Tinder" and "Design Slack", each from 0 % in a newly started Learner, played in 7.6 s and 8.1 s.
- **Before the fix,** on the same machine with decoders installed:
  - `http://127.0.0.1` media failed with error 4 and no connection.
  - `zeron-media://` failed with `No URI handler implemented`.
- All repository gates pass.

## Current limitations

- **Read-ahead:** WebKitGTK's GStreamer player enables on-disk buffering and reads the playing lesson far ahead; "Design Amazon" was 81 % downloaded within about 12 s on loopback. Other lessons are not touched until played.
- **Other platforms:** macOS and Windows still use plain HTTP and are unverified.
- **Packaging:** Linux packages must depend on H.264/AAC GStreamer decoders (for example `gstreamer1.0-libav`).
- **Local and other processes:** the per-launch certificate is trusted only inside the Learner's webview. Other local programs can connect to the port but cannot validate the certificate, and still need the capability URL.
