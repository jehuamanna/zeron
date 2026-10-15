//! Thin Tauri composition root for Zeron Admin Desktop.

mod commands;

use std::sync::{Arc, Mutex};

use publisher_storage::PublisherStorage;
use tauri::Manager;

pub(crate) struct AppState {
    pub(crate) storage: Arc<Mutex<PublisherStorage>>,
}

/// Start the publisher desktop application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or complete its native event loop. A
/// setup failure is logged by Tauri before process termination.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let storage = PublisherStorage::open(&app_data.join("publisher.sqlite3"))?;
            app.manage(AppState {
                storage: Arc::new(Mutex::new(storage)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_drafts,
            commands::create_draft,
            commands::get_draft,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Zeron Publisher");
}
