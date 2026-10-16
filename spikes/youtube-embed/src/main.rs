//! Throwaway spike: does an embedded YouTube IFrame player work inside a Tauri
//! (WebKitGTK) webview, and from which page origin?
//!
//! Opens six windows: {tauri:// page, loopback page, loopback page framed by a
//! tauri:// page} x {youtube.com, youtube-nocookie.com}. Each page reports its
//! progress to a loopback HTTP server, which prints it. Also checks keyless
//! oEmbed metadata from Rust.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PLAYER: &str = include_str!("../ui/player.html");

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let port = listener.local_addr()?.port();
            let base = format!("http://127.0.0.1:{port}");
            let report = format!("{base}/report");
            println!("loopback server {base}");

            let only = std::env::var("SPIKE_ONLY").ok();
            let mut labels = Vec::new();
            for host in ["yt", "nocookie"] {
                let query = |label: &str| {
                    format!("label={label}&host={host}&report={}", enc(&report))
                };
                let direct = format!("direct-{host}");
                let loopback = format!("loopback-{host}");
                let nested = format!("nested-{host}");
                let windows = [
                    (
                        direct.clone(),
                        WebviewUrl::CustomProtocol(
                            format!("tauri://localhost/player.html?{}", query(&direct)).parse()?,
                        ),
                    ),
                    (
                        loopback.clone(),
                        WebviewUrl::External(
                            format!("{base}/player.html?{}", query(&loopback)).parse()?,
                        ),
                    ),
                    (
                        nested.clone(),
                        WebviewUrl::CustomProtocol(
                            format!(
                                "tauri://localhost/index.html?player={}",
                                enc(&format!("{base}/player.html?{}", query(&nested)))
                            )
                            .parse()?,
                        ),
                    ),
                ];
                for (label, url) in windows {
                    if only.as_deref().is_some_and(|o| !label.starts_with(o)) {
                        continue;
                    }
                    WebviewWindowBuilder::new(app, &label, url)
                        .title(&label)
                        .inner_size(660.0, 560.0)
                        .build()?;
                    labels.push(label);
                }
            }

            let handle = app.handle().clone();
            let pending = Arc::new(Mutex::new(labels.into_iter().collect::<HashSet<_>>()));
            tauri::async_runtime::spawn(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let pending = Arc::clone(&pending);
                    let handle = handle.clone();
                    tokio::spawn(async move {
                        let mut buf = vec![0u8; 64 * 1024];
                        let mut len = 0;
                        // Read until the headers and the declared body are in.
                        loop {
                            let n = stream.read(&mut buf[len..]).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            len += n;
                            let text = String::from_utf8_lossy(&buf[..len]);
                            if let Some(end) = text.find("\r\n\r\n") {
                                let want = text[..end]
                                    .lines()
                                    .find_map(|l| {
                                        l.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                    })
                                    .unwrap_or(0);
                                if len >= end + 4 + want {
                                    break;
                                }
                            }
                        }
                        let text = String::from_utf8_lossy(&buf[..len]).to_string();
                        let request_line = text.lines().next().unwrap_or_default().to_owned();
                        let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
                        let (status, ctype, payload) = if request_line.starts_with("GET /player.html") {
                            ("200 OK", "text/html; charset=utf-8", PLAYER.to_owned())
                        } else if request_line.starts_with("POST /report") {
                            println!("REPORT {body}");
                            let mut parts = body.splitn(3, ' ');
                            let label = parts.next().unwrap_or_default().to_owned();
                            let event = parts.next().unwrap_or_default();
                            if matches!(event, "done" | "timeout" | "player_error" | "iframe_api_load_failed") {
                                let mut pending = pending.lock().unwrap();
                                pending.remove(&label);
                                if pending.is_empty() {
                                    let handle = handle.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_millis(500)).await;
                                        println!("all windows finished");
                                        handle.exit(0);
                                    });
                                }
                            }
                            ("204 No Content", "text/plain", String::new())
                        } else {
                            ("404 Not Found", "text/plain", String::new())
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{payload}",
                            payload.len()
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
            });

            tauri::async_runtime::spawn(async {
                let client = reqwest::Client::new();
                for url in [
                    "https://www.youtube.com/watch?v=M7lc1UVf-VE",
                    "https://youtu.be/M7lc1UVf-VE",
                    "https://www.youtube.com/playlist?list=UU_x5XG1OV2P6uZZ5FSM9Ttw",
                ] {
                    let request = format!(
                        "https://www.youtube.com/oembed?format=json&url={}",
                        enc(url)
                    );
                    match client.get(request).send().await {
                        Ok(r) => {
                            let status = r.status();
                            let body = r.text().await.unwrap_or_default();
                            println!("OEMBED {url} {status} {}", &body[..body.len().min(160)]);
                        }
                        Err(e) => println!("OEMBED {url} error {e}"),
                    }
                }
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(70)).await;
                println!("global timeout");
                for (label, _) in handle.webview_windows() {
                    println!("unfinished window {label}");
                }
                handle.exit(1);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("spike app");
}

fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
