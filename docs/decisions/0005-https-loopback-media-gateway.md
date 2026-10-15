# ADR 0005: HTTPS loopback media gateway for WebKitGTK

Status: accepted (2026-10-15, chosen by the user over serving the app from `http://localhost` or deferring Linux playback). Evidence: [Linux in-app playback](../releases/phase-4-linux-in-app-playback.md).

## Context

The Learner page is served from `tauri://localhost`, which WebKitGTK treats as a secure origin. The loopback media gateway served plain `http://127.0.0.1` URLs. On WebKitGTK 2.52 this never played in release builds:

- A `<video>` with an `http://127.0.0.1` source fails with error 4 without opening any connection. Plain HTTP media is refused on a secure page, even from loopback, and no CSP violation is reported.
- A custom URI scheme (`zeron-media://`) fails too. WebKit's media player only accepts listed protocols (`isProtocolAllowed: ... (allowed: no)`), and even after `WEBKIT_GST_ALLOWED_URI_PROTOCOLS` allows it, GStreamer has no source for it (`No URI handler implemented for "zeron-media"`). Custom-scheme responses also cannot stream.
- `https` passes WebKit's protocol check, and WebKitGTK can trust one certificate for one host through `webkit_web_context_allow_tls_certificate_for_host`.

## Decision

On Linux, the Learner starts the gateway with `MediaGateway::start_tls`:

- A self-signed certificate for `127.0.0.1` is generated with `rcgen` at every launch and never stored. TLS uses `rustls` and `tokio-rustls` with the `aws-lc-rs` provider; HTTP/1.1 runs over hyper on the same axum router.
- Authentication, Range handling, verified-only readers, body caps, and cancellation are unchanged.
- At startup the Learner trusts exactly that certificate, for `127.0.0.1` only, in its webview's context. The CSP allows `https://127.0.0.1:*` media.

Other platforms keep `MediaGateway::start` (plain HTTP) until their webviews are verified.

## Consequences

- Media streams natively with Range and seeking; no custom protocol, no chunking, no plain-HTTP page.
- The trust is scoped to one host and one per-launch certificate. Any process on the machine can still connect to the port, as before, but it needs the per-launch capability URL.
- `rcgen` is a new dependency; `rustls`, `tokio-rustls`, `hyper`, and `hyper-util` were already in the tree and are now declared explicitly.
- Linux packaging must also provide H.264/AAC GStreamer decoders (for example `gstreamer1.0-libav`).
