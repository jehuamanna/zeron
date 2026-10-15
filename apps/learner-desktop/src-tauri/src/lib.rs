//! Thin Tauri composition root for Zeron Learner Desktop.

mod commands;
mod media;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use learning_application::TorrentEngine as _;
use learning_application::{
    CourseLibrary, CourseSharing, ImportLimits, NetworkProfile, TorrentImportTimeouts,
    TorrentImports, TransferRouter,
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
    pub(crate) engines: Vec<Arc<LibrqbitEngine>>,
    /// Peer port of the private engine, which downloads and never uploads.
    pub(crate) download_port: Option<u16>,
    /// Peer port of the engine that serves shared courses.
    pub(crate) sharing_port: Option<u16>,
    pub(crate) router: Option<Arc<Router>>,
    pub(crate) imports: Option<Arc<TorrentImports<Router>>>,
    pub(crate) sharing: Option<Arc<CourseSharing<LibrqbitEngine>>>,
}

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
            let library = Arc::new(Mutex::new(library));
            let media_gateway =
                tauri::async_runtime::block_on(MediaGateway::start(GatewayLimits::default()))?;
            let torrents = start_torrents(profile, &app_data, &library);
            if let Some(sharing) = &torrents.sharing {
                sharing.load()?;
            }
            if let Some(imports) = torrents.imports.clone() {
                tauri::async_runtime::spawn(async move {
                    let _ = imports.restore().await;
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
            commands::set_network_profile,
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

fn start_torrents(
    profile: NetworkProfile,
    app_data: &std::path::Path,
    library: &Arc<Mutex<CourseLibrary>>,
) -> Torrents {
    let start = |directory: &str, allow_uploads: bool| {
        tauri::async_runtime::block_on(LibrqbitEngine::start(EngineConfig {
            profile,
            state_directory: app_data.join(directory),
            listen_port: None,
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
            engines: Vec::new(),
            download_port: None,
            sharing_port: None,
            router: None,
            imports: None,
            sharing: None,
        };
    };
    let shared = start("engine-sharing", true);
    let router = Arc::new(TransferRouter::new(Arc::clone(&private), shared.clone()));
    let courses_root: PathBuf = app_data.join("courses");
    let port = |engine: &Arc<LibrqbitEngine>| engine.listen_address().map(|address| address.port());
    Torrents {
        profile,
        download_port: port(&private),
        sharing_port: shared.as_ref().and_then(port),
        engines: std::iter::once(private).chain(shared).collect(),
        router: Some(Arc::clone(&router)),
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
