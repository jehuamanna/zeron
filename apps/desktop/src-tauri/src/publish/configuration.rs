//! Export and import of the publisher configuration: trackers, followed
//! tracker lists, and optionally the app's Network settings. File locations
//! come from native dialogs and never cross IPC.

use std::sync::{Arc, Mutex};

use learning_application::CourseLibrary;
use publisher_application::{
    DraftServiceError, MAX_CONFIGURATION_BYTES, NetworkConfiguration, PublisherConfiguration,
    PublisherDrafts,
};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    PublishState,
    commands::{CommandError, lock_drafts, run_blocking, service_error},
};
use crate::AppState;

/// An imported file waiting for the publisher to apply it.
#[derive(Default)]
pub(crate) struct PendingConfiguration {
    next: u64,
    pending: Option<(String, PublisherConfiguration)>,
}

impl PendingConfiguration {
    fn hold(&mut self, configuration: PublisherConfiguration) -> String {
        self.next += 1;
        let token = format!("configuration_{}", self.next);
        self.pending = Some((token.clone(), configuration));
        token
    }

    fn take(&mut self, token: &str) -> Option<PublisherConfiguration> {
        match self.pending.take() {
            Some((held, configuration)) if held == token => Some(configuration),
            other => {
                self.pending = other;
                None
            }
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkConfigurationDto {
    profile: &'static str,
    peer_port: Option<u16>,
    port_forwarding: bool,
    download_limit_kib: Option<u32>,
    upload_limit_kib: Option<u32>,
}

/// What an imported file would change, for review before applying it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigurationPreviewDto {
    token: String,
    trackers: Vec<String>,
    tracker_lists: Vec<String>,
    network: Option<NetworkConfigurationDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppliedConfigurationDto {
    trackers: usize,
    tracker_lists: usize,
    network_applied: bool,
    /// The network profile or port changed; they take effect after a restart.
    restart_required: bool,
}

impl From<NetworkConfiguration> for NetworkConfigurationDto {
    fn from(network: NetworkConfiguration) -> Self {
        let (download_limit_kib, upload_limit_kib) = network.limits.as_kib_per_second();
        Self {
            profile: network.profile.name(),
            peer_port: network.reachability.port,
            port_forwarding: network.reachability.port_forwarding,
            download_limit_kib,
            upload_limit_kib,
        }
    }
}

/// Save the publisher configuration where the publisher chooses. With
/// `includeNetwork` it also carries the Network settings. Returns `false`
/// when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_publisher_configuration(
    app: AppHandle,
    include_network: bool,
    app_state: State<'_, AppState>,
    state: State<'_, PublishState>,
) -> Result<bool, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let library = Arc::clone(&app_state.library);
    let bytes = run_blocking(move || {
        let network = if include_network {
            Some(network_configuration(&library)?)
        } else {
            None
        };
        Ok(lock_drafts(&drafts)?
            .configuration(network)
            .map_err(|error| service_error("configuration_failed", error))?
            .to_bytes())
    })
    .await?;
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name("zeron-publisher.json")
            .add_filter("Zeron publisher configuration", &["json"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let destination = selected.into_path().map_err(|_| unsupported_location())?;
    run_blocking(move || {
        std::fs::write(&destination, bytes).map_err(|_| {
            CommandError::new(
                "export_failed",
                "the configuration could not be written to the chosen location",
            )
        })
    })
    .await?;
    Ok(true)
}

/// Pick a configuration file and validate it. Nothing changes until
/// [`apply_publisher_configuration`]. Returns `None` when the dialog is
/// cancelled.
#[tauri::command]
pub(crate) async fn choose_publisher_configuration(
    app: AppHandle,
    state: State<'_, PublishState>,
) -> Result<Option<ConfigurationPreviewDto>, CommandError> {
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron publisher configuration", &["json"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| unsupported_location())?;
    let configuration = run_blocking(move || read_configuration(&source)).await?;
    let token = lock_pending(&state.pending_configuration)?.hold(configuration.clone());
    Ok(Some(preview(token, configuration)))
}

/// Apply a reviewed configuration: its trackers and lists replace the
/// current ones, and with `applyNetwork` its Network settings are stored and
/// its speed limits applied at once.
#[tauri::command]
pub(crate) async fn apply_publisher_configuration(
    token: String,
    apply_network: bool,
    app_state: State<'_, AppState>,
    state: State<'_, PublishState>,
) -> Result<AppliedConfigurationDto, CommandError> {
    let configuration = lock_pending(&state.pending_configuration)?
        .take(&token)
        .ok_or_else(|| {
            CommandError::new(
                "configuration_expired",
                "choose the configuration file again",
            )
        })?;
    let drafts = Arc::clone(&state.drafts);
    let library = Arc::clone(&app_state.library);
    let applied = configuration.clone();
    let restart_required = run_blocking(move || {
        apply(
            &mut *lock_drafts(&drafts)?,
            &library,
            &applied,
            apply_network,
        )
    })
    .await?;
    let network = configuration.network.filter(|_| apply_network);
    if let Some(network) = network {
        crate::apply_rate_limits(&app_state.torrents, network.limits)
            .map_err(|error| CommandError::new("settings_failed", error))?;
    }
    Ok(AppliedConfigurationDto {
        trackers: configuration.trackers.len(),
        tracker_lists: configuration.tracker_lists.len(),
        network_applied: network.is_some(),
        restart_required,
    })
}

/// Forget a reviewed configuration without applying it.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn discard_publisher_configuration(
    token: String,
    state: State<'_, PublishState>,
) -> Result<(), CommandError> {
    lock_pending(&state.pending_configuration)?.take(&token);
    Ok(())
}

fn read_configuration(source: &std::path::Path) -> Result<PublisherConfiguration, CommandError> {
    use std::io::Read as _;
    let unreadable = || {
        CommandError::new(
            "configuration_unreadable",
            "the chosen file could not be read",
        )
    };
    let file = std::fs::File::open(source).map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIGURATION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    PublisherConfiguration::parse(&bytes)
        .map_err(|error| service_error("configuration_invalid", error))
}

/// Store the configuration. Returns whether the stored network profile or
/// port changed, which needs a restart.
fn apply(
    drafts: &mut PublisherDrafts,
    library: &Arc<Mutex<CourseLibrary>>,
    configuration: &PublisherConfiguration,
    apply_network: bool,
) -> Result<bool, CommandError> {
    drafts
        .apply_configuration(configuration)
        .map_err(|error| service_error("configuration_failed", error))?;
    let Some(network) = configuration.network.filter(|_| apply_network) else {
        return Ok(false);
    };
    let mut library = lock(library)?;
    let failed =
        |error: learning_application::LibraryError| CommandError::new("settings_failed", error);
    let before = (
        library.network_profile().map_err(failed)?,
        library.peer_reachability().map_err(failed)?,
    );
    library
        .set_network_profile(network.profile)
        .map_err(failed)?;
    library
        .set_peer_reachability(network.reachability)
        .map_err(failed)?;
    library.set_rate_limits(network.limits).map_err(failed)?;
    Ok(before != (network.profile, network.reachability))
}

fn network_configuration(
    library: &Arc<Mutex<CourseLibrary>>,
) -> Result<NetworkConfiguration, CommandError> {
    let library = lock(library)?;
    let failed =
        |error: learning_application::LibraryError| CommandError::new("settings_failed", error);
    Ok(NetworkConfiguration {
        profile: library.network_profile().map_err(failed)?,
        reachability: library.peer_reachability().map_err(failed)?,
        limits: library.rate_limits().map_err(failed)?,
    })
}

fn preview(token: String, configuration: PublisherConfiguration) -> ConfigurationPreviewDto {
    ConfigurationPreviewDto {
        token,
        trackers: configuration.trackers,
        tracker_lists: configuration.tracker_lists,
        network: configuration.network.map(NetworkConfigurationDto::from),
    }
}

fn lock(
    library: &Arc<Mutex<CourseLibrary>>,
) -> Result<std::sync::MutexGuard<'_, CourseLibrary>, CommandError> {
    library.lock().map_err(|_| {
        CommandError::new(
            "library_unavailable",
            "the course library worker is unavailable",
        )
    })
}

fn lock_pending(
    pending: &Mutex<PendingConfiguration>,
) -> Result<std::sync::MutexGuard<'_, PendingConfiguration>, CommandError> {
    pending
        .lock()
        .map_err(|_| service_error("configuration_failed", DraftServiceError::JobActive))
}

fn unsupported_location() -> CommandError {
    CommandError::new(
        "unsupported_file_location",
        "the selected file location is not supported",
    )
}

#[cfg(test)]
mod tests {
    use learning_application::{ImportLimits, NetworkProfile};

    use super::*;

    fn library() -> Arc<Mutex<CourseLibrary>> {
        Arc::new(Mutex::new(
            CourseLibrary::open_in_memory(ImportLimits::default()).unwrap(),
        ))
    }

    #[test]
    fn exported_configuration_imports_into_another_publisher() {
        let mut source = PublisherDrafts::open_in_memory().unwrap();
        source
            .set_trackers(&["udp://tracker.example:6969/announce".to_owned()])
            .unwrap();
        source
            .set_tracker_lists(&["https://lists.example/best.txt".to_owned()])
            .unwrap();
        let source_library = library();
        {
            let mut library = source_library.lock().unwrap();
            library
                .set_network_profile(NetworkProfile::Internet)
                .unwrap();
            library
                .set_peer_reachability(
                    learning_application::PeerReachability::new(Some(51_413), true).unwrap(),
                )
                .unwrap();
        }
        let network = network_configuration(&source_library).unwrap();
        let bytes = source.configuration(Some(network)).unwrap().to_bytes();

        let directory =
            std::env::temp_dir().join(format!("zeron-configuration-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("zeron-publisher.json");
        std::fs::write(&file, &bytes).unwrap();
        let configuration = read_configuration(&file).unwrap();
        let previewed =
            serde_json::to_value(preview("t".to_owned(), configuration.clone())).unwrap();
        assert_eq!(previewed["network"]["profile"], "internet");
        assert_eq!(previewed["network"]["peerPort"], 51_413);
        assert_eq!(
            previewed["trackerLists"][0],
            "https://lists.example/best.txt"
        );

        let mut target = PublisherDrafts::open_in_memory().unwrap();
        let target_library = library();
        assert!(!apply(&mut target, &target_library, &configuration, false).unwrap());
        assert_eq!(
            target.trackers().unwrap(),
            ["udp://tracker.example:6969/announce"]
        );
        assert_eq!(
            target_library.lock().unwrap().network_profile().unwrap(),
            NetworkProfile::LocalNetwork
        );
        assert!(apply(&mut target, &target_library, &configuration, true).unwrap());
        assert_eq!(
            target_library.lock().unwrap().network_profile().unwrap(),
            NetworkProfile::Internet
        );
        assert!(!apply(&mut target, &target_library, &configuration, true).unwrap());

        std::fs::write(&file, b"{\"format\":\"other\"}").unwrap();
        let invalid = read_configuration(&file).unwrap_err();
        assert_eq!(invalid.code(), "invalid_input");
        std::fs::write(&file, vec![b' '; MAX_CONFIGURATION_BYTES + 10]).unwrap();
        assert_eq!(
            read_configuration(&file).unwrap_err().code(),
            "invalid_input"
        );
        let missing = read_configuration(&directory.join("missing.json")).unwrap_err();
        assert_eq!(missing.code(), "configuration_unreadable");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_reviewed_configuration_applies_once_and_only_with_its_token() {
        let mut pending = PendingConfiguration::default();
        let configuration = PublisherConfiguration {
            trackers: Vec::new(),
            tracker_lists: Vec::new(),
            network: None,
        };
        let first = pending.hold(configuration.clone());
        let second = pending.hold(configuration.clone());
        assert_ne!(first, second);
        assert!(pending.take(&first).is_none());
        assert_eq!(pending.take(&second), Some(configuration));
        assert!(pending.take(&second).is_none());
    }
}
