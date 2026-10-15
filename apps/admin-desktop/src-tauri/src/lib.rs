//! Thin Tauri composition root for Zeron Admin Desktop.

mod commands;
mod drops;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use publisher_application::{
    DropInbox, NetworkProfile, PeerReachability, PublishJobs, PublisherDrafts, SeedingService,
};
use tauri::{Manager, RunEvent, WindowEvent};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};

pub(crate) struct AppState {
    pub(crate) drafts: Arc<Mutex<PublisherDrafts>>,
    pub(crate) publish_jobs: Arc<PublishJobs>,
    pub(crate) releases_root: PathBuf,
    pub(crate) distribution: Distribution,
    /// Scanned drops waiting for the publisher to apply them.
    pub(crate) drops: Arc<DropInbox>,
}

/// Seeding is optional at runtime: authoring keeps working if the engine cannot start.
pub(crate) struct Distribution {
    pub(crate) active_profile: NetworkProfile,
    /// The part of the stored reachability the active profile honors.
    pub(crate) active_reachability: PeerReachability,
    pub(crate) engine: Option<Arc<LibrqbitEngine>>,
    pub(crate) seeding: Option<Arc<SeedingService<LibrqbitEngine>>>,
    pub(crate) engine_error: Option<String>,
}

/// Start the publisher desktop application.
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
            let mut drafts = PublisherDrafts::open(&app_data.join("publisher.sqlite3"))?;
            let releases_root = app_data.join("releases");
            drafts.recover_releases(&releases_root)?;
            // A failed retry leaves the release marked as deleting; the
            // publisher can retry from the UI, and the next start tries again.
            if let Err(error) = drafts.recover_release_deletions(&releases_root) {
                eprintln!("finishing interrupted release file deletions failed: {error}");
            }
            let publish_jobs = PublishJobs::new(&app_data.join("staging"));
            publish_jobs.recover()?;
            let active_profile = drafts.network_profile()?;
            let reachability = drafts.peer_reachability()?;
            let drafts = Arc::new(Mutex::new(drafts));
            let distribution = start_distribution(
                active_profile,
                reachability,
                app_data.join("engine"),
                &drafts,
                &releases_root,
            );
            if let Some(seeding) = distribution.seeding.clone() {
                tauri::async_runtime::spawn(async move {
                    let _ = seeding.restore().await;
                });
            }
            app.manage(AppState {
                drafts,
                publish_jobs: Arc::new(publish_jobs),
                releases_root,
                distribution,
                drops: Arc::new(DropInbox::default()),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(drag) = event {
                drops::handle_drag_drop(window, drag);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_drafts,
            commands::create_draft,
            commands::get_draft,
            commands::delete_draft,
            commands::update_draft_metadata,
            commands::edit_draft_structure,
            commands::choose_and_attach_resource,
            commands::inspect_media,
            commands::start_publish_preparation,
            commands::get_publish_job,
            commands::cancel_publish_job,
            commands::activate_release,
            commands::list_releases,
            commands::archive_release,
            commands::delete_release_files,
            drops::apply_drop,
            drops::discard_drop,
            commands::get_release_details,
            commands::export_release_torrent,
            commands::validate_draft,
            commands::get_distribution,
            commands::start_seeding,
            commands::stop_seeding,
            commands::set_network_profile,
            commands::set_peer_reachability,
            commands::set_trackers,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Zeron Publisher");
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit)
            && let Some(engine) = app_handle.state::<AppState>().distribution.engine.clone()
        {
            tauri::async_runtime::block_on(engine.shutdown());
        }
    });
}

fn start_distribution(
    active_profile: NetworkProfile,
    reachability: PeerReachability,
    state_directory: PathBuf,
    drafts: &Arc<Mutex<PublisherDrafts>>,
    releases_root: &std::path::Path,
) -> Distribution {
    let started = tauri::async_runtime::block_on(LibrqbitEngine::start(EngineConfig {
        profile: active_profile,
        state_directory,
        reachability,
        allow_uploads: true,
    }));
    let active_reachability = reachability.for_profile(active_profile);
    match started {
        Ok(engine) => {
            let engine = Arc::new(engine);
            Distribution {
                active_profile,
                active_reachability,
                seeding: Some(Arc::new(SeedingService::new(
                    Arc::clone(drafts),
                    Arc::clone(&engine),
                    releases_root.to_path_buf(),
                ))),
                engine: Some(engine),
                engine_error: None,
            }
        }
        Err(error) => Distribution {
            active_profile,
            active_reachability,
            engine: None,
            seeding: None,
            engine_error: Some(error.to_string()),
        },
    }
}
