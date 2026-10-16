//! Thin Tauri composition root for Zeron Desktop.

mod backup;
mod catalogs;
mod commands;
mod media;
mod navigation;
mod publish;
mod updates;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use learning_application::TorrentEngine as _;
use learning_application::{
    CourseCache, CourseLibrary, CourseSharing, DiskGuard, ImportLimits, NetworkProfile,
    PeerReachability, TorrentImportTimeouts, TorrentImports, TransferRouter,
};
use media_gateway::{GatewayLimits, MediaGateway};
use tauri::{Manager, RunEvent, WindowEvent};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};

pub(crate) struct AppState {
    pub(crate) library: Arc<Mutex<CourseLibrary>>,
    pub(crate) media_gateway: MediaGateway,
    /// The course and capability URL currently playing; revoked when another
    /// is issued or its course is removed.
    pub(crate) current_media: Mutex<Option<(String, String)>>,
    pub(crate) torrents: Torrents,
    /// Where torrent courses download, `app_data/courses`. Never sent over IPC.
    pub(crate) courses_root: PathBuf,
}

pub(crate) type Router = TransferRouter<LibrqbitEngine>;

/// Torrent import is optional at runtime: local courses keep working if the
/// engines cannot start. Two engines run:
///
/// - a **private** engine downloads courses and never uploads;
/// - a **public** engine, on the one fixed port, uploads. It serves the
///   courses the learner shares and seeds published releases.
pub(crate) struct Torrents {
    pub(crate) profile: NetworkProfile,
    /// The part of the stored reachability the sharing engine honors.
    pub(crate) reachability: PeerReachability,
    pub(crate) engines: Vec<Arc<LibrqbitEngine>>,
    /// Peer port of the private engine, which downloads and never uploads.
    pub(crate) download_port: Option<u16>,
    /// Peer port of the public engine.
    pub(crate) sharing_port: Option<u16>,
    /// The public engine, which also seeds releases.
    pub(crate) public: Option<Arc<LibrqbitEngine>>,
    /// Asks the router to open both engines' ports, when forwarding applies.
    pub(crate) port_mapper: Option<port_mapping::PortMapper>,
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

/// Work that runs after startup: restore torrent courses, honor the cache
/// cap, watch free space, and index local courses' text for search.
fn spawn_background_work(torrents: &Torrents, library: &Arc<Mutex<CourseLibrary>>) {
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
    let library = Arc::clone(library);
    tauri::async_runtime::spawn_blocking(move || commands::index_local_texts(&library));
}

/// Open the library and publisher data, start the gateway and both engines,
/// and resume background work.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data)?;
    // The separate Learner and Publisher apps' data moves in once, before
    // anything opens a database here. A failure stops startup rather than
    // opening empty databases over a half-finished import; the next start
    // resumes it.
    match data_import::import_old_data(&data_import::ImportPlan::beside(&app_data)) {
        Ok(data_import::ImportOutcome::Imported { learner, publisher }) => {
            eprintln!("imported earlier Zeron data (library: {learner}, publisher: {publisher})");
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("importing earlier Zeron data stopped: {error}");
            return Err(error.into());
        }
    }
    // A restore staged before the last exit is switched in before any
    // database opens. A failure stops startup; the next start retries.
    if let Err(error) = backup::apply_pending(&app_data) {
        eprintln!("finishing the restore stopped: {error}");
        return Err(error.into());
    }
    let library = CourseLibrary::open(&app_data.join("library.sqlite3"), ImportLimits::default())?;
    // Finish deleting the downloads of courses removed just before a crash.
    let courses_root = app_data.join("courses");
    match learning_application::finish_interrupted_removals(&courses_root) {
        Ok(0) => {}
        Ok(count) => eprintln!("finished removing {count} course download(s)"),
        Err(error) => eprintln!("finishing course removals stopped: {}", error.kind()),
    }
    let profile = library.network_profile()?;
    let reachability = library.peer_reachability()?;
    let library = Arc::new(Mutex::new(library));
    let media_gateway = tauri::async_runtime::block_on(start_media_gateway())?;
    trust_media_gateway(app, &media_gateway);
    let torrents = start_torrents(profile, reachability, &app_data, &library);
    let limits = library.lock().map_or_else(
        |_| Ok(learning_application::RateLimits::default()),
        |library| library.rate_limits(),
    )?;
    if let Err(error) = apply_rate_limits(&torrents, limits) {
        eprintln!("speed limits were not applied: {error}");
    }
    // Releases are reserved on the public engine before the Library's
    // transfers restore, so a learner copy of one stays private.
    let publish =
        publish::PublishState::open(&app_data, torrents.public.clone(), torrents.router.clone())?;
    if let Some(sharing) = &torrents.sharing {
        sharing.load()?;
    }
    spawn_background_work(&torrents, &library);
    publish.spawn_background_work(profile);
    app.manage(publish);
    app.manage(AppState {
        library,
        media_gateway,
        current_media: Mutex::new(None),
        torrents,
        courses_root,
    });
    Ok(())
}

/// Every IPC command the app answers.
#[expect(
    clippy::too_many_lines,
    reason = "one flat list of commands is easiest to audit"
)]
fn ipc_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
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
        commands::read_document,
        commands::read_transcript,
        commands::read_text_resource,
        commands::search_library,
        commands::list_notes,
        commands::get_annotation_moves,
        commands::forget_course_publisher,
        catalogs::list_catalogs,
        backup::export_backup,
        backup::choose_backup_to_restore,
        backup::get_pending_restore,
        backup::cancel_restore,
        backup::restart_app,
        updates::check_for_update,
        updates::install_update,
        catalogs::choose_catalog_file,
        catalogs::import_catalog_url,
        catalogs::refresh_catalog,
        catalogs::remove_catalog,
        commands::move_annotations,
        commands::create_note,
        commands::update_note,
        commands::get_drawing,
        commands::list_drawn_lessons,
        commands::remove_course,
        commands::list_removed_courses,
        commands::save_drawing,
        commands::delete_note,
        commands::set_lesson_completed,
        commands::get_storage,
        commands::set_cache_limit,
        commands::get_playback_position,
        commands::set_network_profile,
        commands::set_peer_reachability,
        commands::set_rate_limits,
        publish::commands::list_drafts,
        publish::commands::create_draft,
        publish::commands::get_draft,
        publish::commands::delete_draft,
        publish::commands::update_draft_metadata,
        publish::commands::edit_draft_structure,
        publish::commands::choose_and_attach_resource,
        publish::commands::inspect_media,
        publish::commands::start_publish_preparation,
        publish::commands::get_publish_job,
        publish::commands::cancel_publish_job,
        publish::commands::discard_publish_job,
        publish::commands::activate_release,
        publish::commands::list_releases,
        publish::commands::archive_release,
        publish::commands::make_release_current,
        publish::commands::get_distribution_activity,
        publish::configuration::export_publisher_configuration,
        publish::catalog::get_catalog,
        publish::catalog::set_catalog_title,
        publish::catalog::set_catalog_release,
        publish::catalog::revoke_retired_key,
        publish::catalog::export_catalog,
        publish::identity::get_publisher_identity,
        publish::identity::create_publisher_identity,
        publish::identity::rotate_publisher_identity,
        publish::identity::set_sign_releases,
        publish::identity::export_identity_backup,
        publish::identity::restore_identity_backup,
        publish::configuration::choose_publisher_configuration,
        publish::configuration::apply_publisher_configuration,
        publish::configuration::discard_publisher_configuration,
        publish::commands::export_distribution_activity,
        publish::commands::create_draft_from_release,
        publish::commands::delete_release_files,
        publish::drops::apply_drop,
        publish::drops::choose_lesson_sources,
        publish::drops::discard_drop,
        publish::commands::get_release_details,
        publish::commands::export_release_torrent,
        publish::commands::validate_draft,
        publish::commands::get_distribution,
        publish::commands::start_seeding,
        publish::commands::stop_seeding,
        publish::commands::set_trackers,
        publish::commands::set_tracker_lists,
        publish::commands::refresh_tracker_lists,
    ]
}

/// Start the Zeron desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or complete its native event loop. A
/// setup failure is logged by Tauri before process termination.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(navigation::guard())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(setup)
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(drag) = event {
                publish::drops::handle_drag_drop(window, drag);
            }
        })
        .invoke_handler(ipc_handlers())
        .build(tauri::generate_context!())
        .expect("failed to build Zeron");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            shut_down(app_handle);
        }
    });
}

/// Stop the gateway, close the router's ports, then stop the engines.
fn shut_down(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    state.media_gateway.shutdown();
    // Close the router's ports before the engines stop listening.
    if let Some(mapper) = state.torrents.port_mapper.clone() {
        tauri::async_runtime::block_on(mapper.shutdown(Duration::from_secs(3)));
    }
    for engine in state.torrents.engines.clone() {
        tauri::async_runtime::block_on(engine.shutdown());
    }
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

/// Only the public engine serves peers, so only it takes the fixed port; the
/// private engine always uses a new port. When forwarding applies, Zeron's
/// own port mapper asks the router to open both ports for TCP and UDP (uTP),
/// so librqbit's TCP-only `UPnP` stays off. Each engine starts on its own, so
/// releases can seed even if the private engine fails, and the reverse.
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
            reachability: PeerReachability {
                port: if allow_uploads {
                    reachability.port
                } else {
                    None
                },
                port_forwarding: false,
            },
            allow_uploads,
        }))
        .map(Arc::new)
        // The detail can name local directories, so it stays in the process log.
        .inspect_err(|error| eprintln!("torrent engine `{directory}` unavailable: {error}"))
        .ok()
    };
    let private = start("engine", false);
    let public = start("engine-sharing", true);
    let port = |engine: &Arc<LibrqbitEngine>| engine.listen_address().map(|address| address.port());
    let engines: Vec<_> = private.iter().chain(public.iter()).cloned().collect();
    let sharing_port = public.as_ref().and_then(port);
    let port_mapper = reachability
        .port_forwarding
        .then(|| start_port_mapper(engines.iter().filter_map(port).collect()));
    let Some(private) = private else {
        return Torrents {
            profile,
            reachability,
            engines,
            download_port: None,
            sharing_port,
            public,
            port_mapper,
            router: None,
            imports: None,
            sharing: None,
            cache: None,
            disk: None,
        };
    };
    let router = Arc::new(TransferRouter::new(Arc::clone(&private), public.clone()));
    let courses_root: PathBuf = app_data.join("courses");
    Torrents {
        profile,
        reachability,
        download_port: port(&private),
        sharing_port,
        public,
        port_mapper,
        engines,
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

/// Keep `ports` open on the router for as long as Zeron runs.
fn start_port_mapper(ports: Vec<u16>) -> port_mapping::PortMapper {
    let (mapper, run) = port_mapping::PortMapper::new(port_mapping::MapperConfig::default(), ports);
    tauri::async_runtime::spawn(run);
    mapper
}

/// Apply speed caps to every running engine: through the router when the
/// private engine runs, else to the public engine alone.
pub(crate) fn apply_rate_limits(
    torrents: &Torrents,
    limits: learning_application::RateLimits,
) -> Result<(), learning_application::EngineError> {
    match (&torrents.router, &torrents.public) {
        (Some(router), _) => router.set_rate_limits(limits),
        (None, Some(public)) => public.set_rate_limits(limits),
        (None, None) => Ok(()),
    }
}
