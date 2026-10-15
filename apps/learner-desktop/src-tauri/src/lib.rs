//! Thin Tauri composition root for Zeron Learner Desktop.

mod commands;

use std::sync::{Arc, Mutex};

use learning_application::{CourseLibrary, ImportLimits};
use tauri::Manager;

pub(crate) struct AppState {
    pub(crate) library: Arc<Mutex<CourseLibrary>>,
}

/// Start the learner desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or complete its native event loop. A
/// setup failure is logged by Tauri before process termination.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let library =
                CourseLibrary::open(&app_data.join("library.sqlite3"), ImportLimits::default())?;
            app.manage(AppState {
                library: Arc::new(Mutex::new(library)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::readiness,
            commands::list_courses,
            commands::choose_and_import_course,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Zeron Learner Desktop");
}
