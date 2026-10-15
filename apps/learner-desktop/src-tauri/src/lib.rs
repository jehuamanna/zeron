//! Thin Tauri composition root for Zeron Learner Desktop.

mod commands;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use learning_application::{
    CourseLibrary, ImportLimits, NetworkProfile, TorrentImportTimeouts, TorrentImports,
};
use media_gateway::{GatewayLimits, MediaGateway};
use tauri::{Manager, RunEvent};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};

pub(crate) struct AppState {
    pub(crate) library: Arc<Mutex<CourseLibrary>>,
    pub(crate) media_gateway: MediaGateway,
    pub(crate) torrents: Torrents,
}

/// Torrent import is optional at runtime: local courses keep working if the
/// engine cannot start.
pub(crate) struct Torrents {
    pub(crate) engine: Option<Arc<LibrqbitEngine>>,
    pub(crate) imports: Option<Arc<TorrentImports<LibrqbitEngine>>>,
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
            if let Some(imports) = torrents.imports.clone() {
                tauri::async_runtime::spawn(async move {
                    let _ = imports.restore().await;
                });
            }
            app.manage(AppState {
                library,
                media_gateway,
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
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Zeron Learner Desktop");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            let state = app_handle.state::<AppState>();
            state.media_gateway.shutdown();
            if let Some(engine) = state.torrents.engine.clone() {
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
    let started = tauri::async_runtime::block_on(LibrqbitEngine::start(EngineConfig {
        profile,
        state_directory: app_data.join("engine"),
        listen_port: None,
    }));
    match started {
        Ok(engine) => {
            let engine = Arc::new(engine);
            let courses_root: PathBuf = app_data.join("courses");
            Torrents {
                imports: Some(Arc::new(TorrentImports::new(
                    Arc::clone(&engine),
                    Arc::clone(library),
                    courses_root,
                    ImportLimits::default(),
                    TorrentImportTimeouts::default(),
                ))),
                engine: Some(engine),
            }
        }
        Err(error) => {
            // The detail can name local directories, so it stays in the process log.
            eprintln!("torrent engine unavailable: {error}");
            Torrents {
                engine: None,
                imports: None,
            }
        }
    }
}
