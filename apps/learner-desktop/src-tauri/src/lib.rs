//! Thin Tauri composition root for Zeron Learner Desktop.

mod commands;
mod media;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use learning_application::TorrentEngine as _;
use learning_application::{
    CourseCache, CourseLibrary, CourseSharing, DiskGuard, ImportLimits, NetworkProfile,
    PeerReachability, TorrentImportTimeouts, TorrentImports, TransferRouter,
};
use media_gateway::{GatewayLimits, MediaGateway};
use tauri::{Manager, RunEvent};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};

pub(crate) struct AppState {
    pub(crate) library: Arc<Mutex<CourseLibrary>>,
    pub(crate) media_gateway: MediaGateway,
    /// The capability URL currently playing; revoked when another is issued.
    pub(crate) current_media: Mutex<Option<String>>,
    pub(crate) torrents: Torrents,
}

pub(crate) type Router = TransferRouter<LibrqbitEngine>;

/// Torrent import is optional at runtime: local courses keep working if the
/// engines cannot start. Courses stay in a private engine that never uploads
/// unless the learner shares them; shared courses move to a second engine.
pub(crate) struct Torrents {
    pub(crate) profile: NetworkProfile,
    /// The part of the stored reachability the sharing engine honors.
    pub(crate) reachability: PeerReachability,
    pub(crate) engines: Vec<Arc<LibrqbitEngine>>,
    /// Peer port of the private engine, which downloads and never uploads.
    pub(crate) download_port: Option<u16>,
    /// Peer port of the engine that serves shared courses.
    pub(crate) sharing_port: Option<u16>,
    pub(crate) router: Option<Arc<Router>>,
    pub(crate) imports: Option<Arc<TorrentImports<Router>>>,
    pub(crate) sharing: Option<Arc<CourseSharing<LibrqbitEngine>>>,
    /// Removes streamed lessons the learner does not keep offline.
    pub(crate) cache: Option<Arc<CourseCache<Router>>>,
    /// Pauses downloads while the disk is nearly full.
    pub(crate) disk: Option<Arc<DiskGuard<Router>>>,
}

/// How often free disk space is checked while downloads may run.
const DISK_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Start the learner desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or complete its native event loop. A
/// setup failure is logged by Tauri before process termination.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let library =
                CourseLibrary::open(&app_data.join("library.sqlite3"), ImportLimits::default())?;
            let profile = library.network_profile()?;
            let reachability = library.peer_reachability()?;
            let library = Arc::new(Mutex::new(library));
            let media_gateway = tauri::async_runtime::block_on(start_media_gateway())?;
            trust_media_gateway(app, &media_gateway);
            let torrents = start_torrents(profile, reachability, &app_data, &library);
            if let Some(router) = &torrents.router {
                let limits = library.lock().map_or_else(
                    |_| Ok(learning_application::RateLimits::default()),
                    |library| library.rate_limits(),
                )?;
                if let Err(error) = router.set_rate_limits(limits) {
                    eprintln!("speed limits were not applied: {error}");
                }
            }
            if let Some(sharing) = &torrents.sharing {
                sharing.load()?;
            }
            if let Some(imports) = torrents.imports.clone() {
                let cache = torrents.cache.clone();
                let disk = torrents.disk.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = imports.restore().await;
                    // Honor the cache cap once restored courses report their bytes.
                    if let Some(cache) = cache {
                        let _ = cache.enforce_limit(None).await;
                    }
                    // Keep watching free space for as long as the app runs.
                    if let Some(disk) = disk {
                        loop {
                            disk.check().await;
                            tokio::time::sleep(DISK_CHECK_INTERVAL).await;
                        }
                    }
                });
            }
            app.manage(AppState {
                library,
                media_gateway,
                current_media: Mutex::new(None),
                torrents,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::readiness,
            commands::list_courses,
            commands::get_course_detail,
            commands::register_media,
            commands::choose_and_import_course,
            commands::choose_torrent_file_and_import,
            commands::import_magnet,
            commands::get_torrent_import,
            commands::cancel_torrent_import,
            commands::get_course_sharing,
            commands::set_course_sharing,
            commands::get_network_settings,
            commands::get_course_availability,
            commands::save_playback_position,
            commands::get_course_downloads,
            commands::set_course_download,
            commands::set_course_paused,
            commands::remove_downloads,
            commands::get_course_progress,
            commands::set_lesson_completed,
            commands::get_storage,
            commands::set_cache_limit,
            commands::get_playback_position,
            commands::set_network_profile,
            commands::set_peer_reachability,
            commands::set_rate_limits,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Zeron Learner Desktop");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            let state = app_handle.state::<AppState>();
            state.media_gateway.shutdown();
            for engine in state.torrents.engines.clone() {
                tauri::async_runtime::block_on(engine.shutdown());
            }
        }
    });
}

/// `WebKitGTK` refuses plain HTTP media on the secure `tauri://localhost`
/// page and plays only http, https, and blob URLs, so on Linux the gateway
/// serves HTTPS with a per-launch certificate that the webview trusts.
#[cfg(target_os = "linux")]
async fn start_media_gateway() -> Result<MediaGateway, media_gateway::GatewayError> {
    MediaGateway::start_tls(GatewayLimits::default()).await
}

#[cfg(not(target_os = "linux"))]
async fn start_media_gateway() -> Result<MediaGateway, media_gateway::GatewayError> {
    MediaGateway::start(GatewayLimits::default()).await
}

/// Trust exactly this launch's gateway certificate, for `127.0.0.1` only.
#[cfg(target_os = "linux")]
fn trust_media_gateway(app: &tauri::App, gateway: &MediaGateway) {
    use webkit2gtk::{WebContextExt as _, WebViewExt as _};

    let Some(pem) = gateway.certificate_pem().map(str::to_owned) else {
        return;
    };
    for window in app.webview_windows().into_values() {
        let pem = pem.clone();
        let _ = window.with_webview(move |webview| {
            let certificate = webkit2gtk::gio::TlsCertificate::from_pem(&pem);
            if let (Ok(certificate), Some(context)) = (certificate, webview.inner().context()) {
                context.allow_tls_certificate_for_host(&certificate, "127.0.0.1");
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn trust_media_gateway(_: &tauri::App, _: &MediaGateway) {}

/// Only the sharing engine serves peers, so only it takes the learner's fixed
/// port and router forwarding; the private engine always uses a new port.
fn start_torrents(
    profile: NetworkProfile,
    reachability: PeerReachability,
    app_data: &std::path::Path,
    library: &Arc<Mutex<CourseLibrary>>,
) -> Torrents {
    let reachability = reachability.for_profile(profile);
    let start = |directory: &str, allow_uploads: bool| {
        tauri::async_runtime::block_on(LibrqbitEngine::start(EngineConfig {
            profile,
            state_directory: app_data.join(directory),
            reachability: if allow_uploads {
                reachability
            } else {
                PeerReachability::default()
            },
            allow_uploads,
        }))
        .map(Arc::new)
        // The detail can name local directories, so it stays in the process log.
        .inspect_err(|error| eprintln!("torrent engine `{directory}` unavailable: {error}"))
        .ok()
    };
    let Some(private) = start("engine", false) else {
        return Torrents {
            profile,
            reachability,
            engines: Vec::new(),
            download_port: None,
            sharing_port: None,
            router: None,
            imports: None,
            sharing: None,
            cache: None,
            disk: None,
        };
    };
    let shared = start("engine-sharing", true);
    let router = Arc::new(TransferRouter::new(Arc::clone(&private), shared.clone()));
    let courses_root: PathBuf = app_data.join("courses");
    let port = |engine: &Arc<LibrqbitEngine>| engine.listen_address().map(|address| address.port());
    Torrents {
        profile,
        reachability,
        download_port: port(&private),
        sharing_port: shared.as_ref().and_then(port),
        engines: std::iter::once(private).chain(shared).collect(),
        router: Some(Arc::clone(&router)),
        cache: Some(Arc::new(CourseCache::new(
            Arc::clone(&router),
            Arc::clone(library),
            courses_root.clone(),
        ))),
        disk: Some(Arc::new(DiskGuard::new(
            Arc::clone(&router),
            Arc::clone(library),
            courses_root.clone(),
        ))),
        imports: Some(Arc::new(TorrentImports::new(
            Arc::clone(&router),
            Arc::clone(library),
            courses_root,
            ImportLimits::default(),
            TorrentImportTimeouts::default(),
        ))),
        sharing: Some(Arc::new(CourseSharing::new(router, Arc::clone(library)))),
    }
}
