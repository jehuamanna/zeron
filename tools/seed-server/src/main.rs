//! `zeron-seed`: command-line entry point of the seed server.

use std::{
    error::Error,
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use seed_server::{
    registry::{MAX_TORRENT_BYTES, Registry, Source},
    server::{ServeConfig, Server, serve_health},
};
use torrent_port::{NetworkProfile, RateLimits};

const DEFAULT_DATA: &str = "/var/lib/zeron-seed";
const DEFAULT_HEALTH: &str = "127.0.0.1:8571";

fn usage() -> &'static str {
    "usage: zeron-seed [--data DIR] <command>

commands:
  serve [--port N | --any-port] [--profile internet|local_network|loopback]
        [--upload-limit KiB/s] [--download-limit KiB/s] [--map-ports]
        [--health ADDR | --no-health]
      Run the server until SIGINT or SIGTERM.
  add <magnet> [--peer IP:PORT]...
      Follow a release: download it from its swarm, then serve it.
  add-release <release-dir>
  add-release --torrent FILE --content DIR
      Serve a release copied to this server (read-only).
  remove <info-hash>
      Stop serving a torrent; downloaded files are kept.
  list
      Show the seed list.
  health [--health ADDR]
      Ask a running server how it is; exits 1 if it does not answer.

--data defaults to $ZERON_SEED_DATA or /var/lib/zeron-seed.
--health defaults to 127.0.0.1:8571."
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(arguments)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Arguments as UTF-8 strings, consumed from the front.
struct Arguments(std::collections::VecDeque<String>);

impl Arguments {
    fn next(&mut self) -> Option<String> {
        self.0.pop_front()
    }

    fn value(&mut self, flag: &str) -> Result<String, String> {
        self.next().ok_or_else(|| format!("{flag} needs a value"))
    }
}

async fn run(arguments: Vec<OsString>) -> Result<(), Box<dyn Error>> {
    let strings = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "arguments must be UTF-8")
        })
        .collect::<Result<_, _>>()?;
    let mut arguments = Arguments(strings);
    let mut data = std::env::var_os("ZERON_SEED_DATA")
        .map_or_else(|| PathBuf::from(DEFAULT_DATA), PathBuf::from);
    let command = loop {
        match arguments.next().as_deref() {
            Some("--data") => data = PathBuf::from(arguments.value("--data")?),
            Some("-h" | "--help") | None => {
                println!("{}", usage());
                return Ok(());
            }
            Some(command) => break command.to_owned(),
        }
    };
    match command.as_str() {
        "serve" => serve(&data, arguments).await,
        "add" => add(&data, arguments),
        "add-release" => add_release(&data, arguments),
        "remove" => {
            let hash = arguments.next().ok_or("remove needs an info hash")?;
            if Registry::open(&data)?.remove(&hash)? {
                println!("removed {hash}; its files are kept");
                Ok(())
            } else {
                Err(format!("{hash} is not in the seed list").into())
            }
        }
        "list" => {
            for entry in Registry::open(&data)?.entries()? {
                match entry.source {
                    Source::Follow { magnet, peers } => {
                        println!(
                            "{}  follow  {magnet}{}",
                            entry.info_hash,
                            if peers.is_empty() {
                                String::new()
                            } else {
                                format!("  peers: {peers:?}")
                            }
                        );
                    }
                    Source::Release { content_root } => {
                        println!("{}  release  {}", entry.info_hash, content_root.display());
                    }
                }
            }
            Ok(())
        }
        "health" => {
            let address: SocketAddr = match arguments.next().as_deref() {
                Some("--health") => arguments.value("--health")?.parse()?,
                Some(other) => return Err(format!("unknown option {other}").into()),
                None => DEFAULT_HEALTH.parse()?,
            };
            println!("{}", fetch_health(address).await?);
            Ok(())
        }
        other => Err(format!("unknown command {other}\n\n{}", usage()).into()),
    }
}

async fn serve(data: &Path, mut arguments: Arguments) -> Result<(), Box<dyn Error>> {
    let mut config = ServeConfig::default();
    let mut health: Option<SocketAddr> = Some(DEFAULT_HEALTH.parse()?);
    let (mut upload, mut download) = (None, None);
    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--port" => config.port = Some(arguments.value("--port")?.parse()?),
            "--any-port" => config.port = None,
            "--profile" => {
                config.profile = NetworkProfile::from_name(&arguments.value("--profile")?)
                    .ok_or("--profile is internet, local_network, or loopback")?;
            }
            "--upload-limit" => upload = Some(arguments.value("--upload-limit")?.parse()?),
            "--download-limit" => download = Some(arguments.value("--download-limit")?.parse()?),
            "--map-ports" => config.map_ports = true,
            "--health" => health = Some(arguments.value("--health")?.parse()?),
            "--no-health" => health = None,
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    if config.port.is_some_and(|port| port < 1024) {
        return Err("choose a --port from 1024 to 65535".into());
    }
    config.limits = RateLimits::from_kib_per_second(download, upload)
        .ok_or("speed limits must be at least 16 KiB/s, or 0 for none")?;
    let poll = config.poll;
    let server = Server::start(Registry::open(data)?, config).await?;
    eprintln!(
        "serving peers on {}",
        server
            .listen_address()
            .map_or_else(|| "no port".to_owned(), |address| address.to_string())
    );
    let (stop, stopped) = tokio::sync::watch::channel(false);
    if let Some(address) = health {
        let mut stopped = stopped.clone();
        let bound = serve_health(Arc::clone(&server), address, async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        })
        .await?;
        eprintln!("health on http://{bound}/health");
    }
    let mut interval = tokio::time::interval(poll);
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            _ = interval.tick() => {
                if let Err(error) = server.reconcile().await {
                    eprintln!("reading the seed list failed: {error}");
                }
            }
        }
    }
    eprintln!("shutting down");
    let _ = stop.send(true);
    server.shutdown().await;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let Ok(mut terminate) = signal(SignalKind::terminate()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn add(data: &Path, mut arguments: Arguments) -> Result<(), Box<dyn Error>> {
    let magnet = arguments.next().ok_or("add needs a magnet link")?;
    let mut peers = Vec::new();
    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--peer" => peers.push(arguments.value("--peer")?.parse()?),
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    let entry = Registry::open(data)?.add_magnet(&magnet, peers)?;
    println!(
        "following {}; a running server picks it up within seconds",
        entry.info_hash
    );
    Ok(())
}

fn add_release(data: &Path, mut arguments: Arguments) -> Result<(), Box<dyn Error>> {
    let (torrent, content) = match arguments.next() {
        Some(flag) if flag == "--torrent" => {
            let torrent = PathBuf::from(arguments.value("--torrent")?);
            match arguments.next().as_deref() {
                Some("--content") => (torrent, PathBuf::from(arguments.value("--content")?)),
                _ => return Err("add-release --torrent FILE needs --content DIR".into()),
            }
        }
        // A release directory as Zeron publishes it.
        Some(directory) => {
            let directory = PathBuf::from(directory);
            (directory.join("release.torrent"), directory.join("package"))
        }
        None => return Err("add-release needs a release directory".into()),
    };
    let length = std::fs::metadata(&torrent)
        .map_err(|error| format!("{}: {error}", torrent.display()))?
        .len();
    if length > MAX_TORRENT_BYTES {
        return Err("the .torrent file is too large".into());
    }
    let bytes = std::fs::read(&torrent)?;
    let info_hash = torrent_metainfo::info_hash(&bytes)
        .map_err(|error| format!("{}: {error}", torrent.display()))?;
    let entry = Registry::open(data)?.add_release(&info_hash, &bytes, &content)?;
    println!(
        "serving release {}; a running server picks it up within seconds",
        entry.info_hash
    );
    Ok(())
}

/// GET the health endpoint with a plain HTTP/1.1 request.
async fn fetch_health(address: SocketAddr) -> Result<String, Box<dyn Error>> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect(address).await?;
        stream
            .write_all(
                format!("GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        stream.take(1024 * 1024).read_to_end(&mut response).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&response).into_owned())
    };
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), exchange)
        .await
        .map_err(|_| "the server did not answer")??;
    let (head, body) = response.split_once("\r\n\r\n").ok_or("malformed answer")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("the server answered: {}", head.lines().next().unwrap_or("")).into());
    }
    Ok(body.to_owned())
}
