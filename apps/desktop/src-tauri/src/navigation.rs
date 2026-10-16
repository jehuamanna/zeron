//! Keeps the window on the app's own pages.
//!
//! Embedded tools such as the drawing editor carry links to web sites. The
//! learner window must never become a browser for remote content, so any
//! navigation away from the bundled app is refused. On Linux this applies to
//! frames too, so the only exceptions are the YouTube player (ADR 0018): the
//! media gateway's registered player page and YouTube's embed frame in it.

use tauri::{Manager as _, Runtime, Url, plugin::TauriPlugin};

use crate::{AppState, youtube};

/// A plugin refusing navigation to anything but the bundled app (and the
/// Vite dev server in debug builds) and the YouTube player's frames.
pub(crate) fn guard<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("navigation-guard")
        .on_navigation(|webview, url| {
            is_app_page(url)
                || youtube::is_embed_frame(url)
                || webview
                    .try_state::<AppState>()
                    .is_some_and(|state| state.media_gateway.is_page_url(url.as_str()))
        })
        .build()
}

fn is_app_page(url: &Url) -> bool {
    match (url.scheme(), url.host_str()) {
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost")) => true,
        ("http", Some("localhost")) => cfg!(debug_assertions) && url.port() == Some(1420),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bundled_pages_are_reachable() {
        for allowed in [
            "tauri://localhost/",
            "tauri://localhost/index.html#x",
            "http://tauri.localhost/",
        ] {
            assert!(is_app_page(&Url::parse(allowed).unwrap()), "{allowed}");
        }
        for refused in [
            "https://libraries.excalidraw.com/",
            "https://www.youtube.com/embed/x",
            "http://127.0.0.1:4000/",
            "http://localhost:8080/",
            "file:///etc/passwd",
            "tauri://evil/",
            "about:blank",
        ] {
            assert!(!is_app_page(&Url::parse(refused).unwrap()), "{refused}");
        }
        assert_eq!(
            is_app_page(&Url::parse("http://localhost:1420/").unwrap()),
            cfg!(debug_assertions)
        );
    }
}
