//! Updates, only when the user asks. Every update is verified against the
//! release signing key in `tauri.conf.json` before it is installed.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::commands::{CommandError, command_error};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateDto {
    version: String,
    current_version: String,
    notes: Option<String>,
}

/// Ask the release server whether a newer Zeron exists.
#[tauri::command]
pub(crate) async fn check_for_update(app: AppHandle) -> Result<Option<UpdateDto>, CommandError> {
    let update = app
        .updater()
        .map_err(|error| command_error("update_unavailable", error))?
        .check()
        .await
        .map_err(update_error)?;
    Ok(update.map(|update| UpdateDto {
        version: update.version,
        current_version: update.current_version,
        notes: update.body,
    }))
}

/// Download, verify, and install the newer Zeron, then restart into it.
#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> Result<(), CommandError> {
    let update = app
        .updater()
        .map_err(|error| command_error("update_unavailable", error))?
        .check()
        .await
        .map_err(update_error)?
        .ok_or_else(|| command_error("no_update", "Zeron is up to date"))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(update_error)?;
    app.restart();
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a map_err adapter"
)]
fn update_error(error: tauri_plugin_updater::Error) -> CommandError {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_) | Error::SignatureUtf8(_) => command_error(
            "update_not_trusted",
            "the update is not signed by Zeron's release key, so it was not installed",
        ),
        Error::UnsupportedOs | Error::UnsupportedArch | Error::TargetNotFound(_) => command_error(
            "update_unavailable",
            "no update is published for this kind of computer",
        ),
        // Network errors can carry addresses; keep the reason short.
        _ => command_error(
            "update_failed",
            "the update could not be checked or downloaded",
        ),
    }
}
