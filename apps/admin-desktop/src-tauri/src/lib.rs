//! Thin Tauri composition root for Zeron Admin Desktop.

mod commands;

use std::sync::{Arc, Mutex};

use publisher_application::PublisherDrafts;
use tauri::Manager;

pub(crate) struct AppState {
    pub(crate) drafts: Arc<Mutex<PublisherDrafts>>,
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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let drafts = PublisherDrafts::open(&app_data.join("publisher.sqlite3"))?;
            app.manage(AppState {
                drafts: Arc::new(Mutex::new(drafts)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_drafts,
            commands::create_draft,
            commands::get_draft,
            commands::update_draft_metadata,
            commands::edit_draft_structure,
            commands::choose_and_attach_resource,
            commands::validate_draft,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Zeron Publisher");
}
