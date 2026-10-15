//! Thin Tauri composition root for Zeron Learner Desktop.

mod commands;

use std::sync::{Arc, Mutex};

use learning_application::{CourseLibrary, ImportLimits};
use media_gateway::{GatewayLimits, MediaGateway};
use tauri::{Manager, RunEvent};

pub(crate) struct AppState {
    pub(crate) library: Arc<Mutex<CourseLibrary>>,
    pub(crate) media_gateway: MediaGateway,
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
            let media_gateway =
                tauri::async_runtime::block_on(MediaGateway::start(GatewayLimits::default()))?;
            app.manage(AppState {
                library: Arc::new(Mutex::new(library)),
                media_gateway,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::readiness,
            commands::list_courses,
            commands::get_course_detail,
            commands::register_media,
            commands::choose_and_import_course,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Zeron Learner Desktop");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            app_handle.state::<AppState>().media_gateway.shutdown();
        }
    });
}
