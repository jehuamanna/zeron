//! Thin Tauri composition root for Zeron Admin Desktop.

mod commands;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use publisher_application::{
    NetworkProfile, PeerReachability, PublishJobs, PublisherDrafts, SeedingService,
};
use tauri::{Manager, RunEvent};
use torrent_librqbit::{EngineConfig, LibrqbitEngine};

pub(crate) struct AppState {
    pub(crate) drafts: Arc<Mutex<PublisherDrafts>>,
    pub(crate) publish_jobs: Arc<PublishJobs>,
    pub(crate) releases_root: PathBuf,
    pub(crate) distribution: Distribution,
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
            let drafts = PublisherDrafts::open(&app_data.join("publisher.sqlite3"))?;
            let releases_root = app_data.join("releases");
            drafts.recover_releases(&releases_root)?;
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
            commands::inspect_media,
            commands::start_publish_preparation,
            commands::get_publish_job,
            commands::cancel_publish_job,
            commands::activate_release,
            commands::list_releases,
            commands::validate_draft,
            commands::get_distribution,
            commands::start_seeding,
            commands::stop_seeding,
            commands::set_network_profile,
            commands::set_peer_reachability,
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
