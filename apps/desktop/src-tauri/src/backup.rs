//! Back up and restore Zeron's databases. Backup files are chosen in native
//! dialogs and stay in Rust; a restore is staged now and switched in when
//! Zeron next starts, before any database opens.

use std::{fs::File, io::BufWriter, path::PathBuf};

use data_backup::{BackupEntry, Database, apply_pending_restore, stage_restore, write_backup};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::commands::{CommandError, command_error, run_blocking};

/// The databases a backup holds, with the newest schema this build opens.
pub(crate) const DATABASES: [Database; 2] = [
    Database {
        name: "library.sqlite3",
        schema_version: learning_storage::CURRENT_SCHEMA_VERSION,
    },
    Database {
        name: "publisher.sqlite3",
        schema_version: publisher_storage::SCHEMA_VERSION,
    },
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackupDto {
    created_at_unix_seconds: Option<u64>,
    /// `library` and `publish` for the databases it holds.
    contents: Vec<&'static str>,
    total_bytes: u64,
}

fn describe(created_at: Option<u64>, entries: &[BackupEntry]) -> BackupDto {
    BackupDto {
        created_at_unix_seconds: created_at,
        contents: entries
            .iter()
            .map(|entry| {
                if entry.name == DATABASES[0].name {
                    "library"
                } else {
                    "publish"
                }
            })
            .collect(),
        total_bytes: entries.iter().map(|entry| entry.length).sum(),
    }
}

/// Startup: switch in a restore staged before the last exit.
pub(crate) fn apply_pending(app_data: &std::path::Path) -> Result<(), data_backup::BackupError> {
    if let Some(aside) = apply_pending_restore(app_data, now())? {
        eprintln!(
            "restored Zeron data from a backup; the previous databases are in {}",
            aside
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    Ok(())
}

/// Save a backup of the Library's and Publish's databases. Returns `None`
/// when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_backup(app: AppHandle) -> Result<Option<BackupDto>, CommandError> {
    let app_data = app_data(&app)?;
    let picker = app.clone();
    let selected = run_blocking(move || {
        Ok(picker
            .dialog()
            .file()
            .set_file_name(format!("zeron-backup-{}.zeronbackup", now()))
            .add_filter("Zeron backup", &["zeronbackup"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let destination = selected.into_path().map_err(|_| unsupported_location())?;
    run_blocking(move || {
        let failed = |_| {
            command_error(
                "backup_failed",
                "the backup could not be written to the chosen location",
            )
        };
        let partial = destination.with_extension("zeronbackup.partial");
        let file = File::create(&partial).map_err(failed)?;
        let mut writer = BufWriter::new(file);
        let summary = write_backup(&app_data, &DATABASES, now(), &mut writer)
            .map_err(|error| command_error("backup_failed", error));
        drop(writer);
        let summary = match summary {
            Ok(summary) => summary,
            Err(error) => {
                let _ = std::fs::remove_file(&partial);
                return Err(error);
            }
        };
        std::fs::rename(&partial, &destination).map_err(failed)?;
        Ok(Some(describe(Some(summary.created_at), &summary.entries)))
    })
    .await
}

/// Check a backup chosen in a native dialog and stage it for the next
/// start. Returns `None` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn choose_backup_to_restore(
    app: AppHandle,
) -> Result<Option<BackupDto>, CommandError> {
    let app_data = app_data(&app)?;
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron backup", &["zeronbackup"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| unsupported_location())?;
    run_blocking(move || stage(&app_data, &source).map(Some)).await
}

/// The restore waiting for the next start, if any.
#[tauri::command]
pub(crate) async fn get_pending_restore(app: AppHandle) -> Result<Option<BackupDto>, CommandError> {
    let app_data = app_data(&app)?;
    run_blocking(move || {
        data_backup::pending_restore(&app_data)
            .map(|entries| entries.map(|entries| describe(None, &entries)))
            .map_err(|error| command_error("restore_failed", error))
    })
    .await
}

/// Forget a staged restore.
#[tauri::command]
pub(crate) async fn cancel_restore(app: AppHandle) -> Result<(), CommandError> {
    let app_data = app_data(&app)?;
    run_blocking(move || {
        data_backup::discard_pending_restore(&app_data)
            .map_err(|error| command_error("restore_failed", error))
    })
    .await
}

/// Restart Zeron, for example to finish a restore.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn restart_app(app: AppHandle) {
    app.restart();
}

fn stage(app_data: &std::path::Path, source: &std::path::Path) -> Result<BackupDto, CommandError> {
    let mut file = File::open(source)
        .map_err(|_| command_error("backup_unreadable", "the chosen file could not be read"))?;
    let summary = stage_restore(app_data, &DATABASES, &mut file).map_err(|error| match error {
        data_backup::BackupError::TooNew => command_error("backup_too_new", error),
        data_backup::BackupError::Invalid(_) => command_error("backup_invalid", error),
        _ => command_error("restore_failed", error),
    })?;
    Ok(describe(Some(summary.created_at), &summary.entries))
}

fn app_data(app: &AppHandle) -> Result<PathBuf, CommandError> {
    app.path()
        .app_data_dir()
        .map_err(|_| command_error("restore_failed", "the data folder is unavailable"))
}

fn unsupported_location() -> CommandError {
    command_error(
        "unsupported_file_location",
        "the selected file location is not supported",
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_name_their_contents_and_refusals_have_codes() {
        let entries = vec![
            BackupEntry {
                name: "library.sqlite3".to_owned(),
                length: 10,
                sha256: String::new(),
                schema_version: 1,
            },
            BackupEntry {
                name: "publisher.sqlite3".to_owned(),
                length: 5,
                sha256: String::new(),
                schema_version: 1,
            },
        ];
        let json = serde_json::to_value(describe(Some(3), &entries)).unwrap();
        assert_eq!(json["contents"], serde_json::json!(["library", "publish"]));
        assert_eq!(json["totalBytes"], 15);

        let directory =
            std::env::temp_dir().join(format!("zeron-backup-cmd-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("not-a-backup.zeronbackup");
        std::fs::write(&file, b"hello").unwrap();
        let refused = stage(&directory, &file).unwrap_err();
        assert_eq!(refused.code, "backup_invalid");
        assert!(!refused.message.contains(&*directory.to_string_lossy()));
        assert_eq!(
            stage(&directory, &directory.join("missing"))
                .unwrap_err()
                .code,
            "backup_unreadable"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
