//! Importing a license file into the Library (ADR 0017). The file is
//! verified offline against the course's pinned publisher and this
//! computer's learner key, then kept as learner data.

use std::sync::Arc;

use course_signing::{MAX_LICENSE_BYTES, PublicKey};
use learning_application::{CourseLibrary, ImportedLicense, LibraryError};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    AppState,
    commands::{CommandError, command_error, lock_library, run_blocking},
    learner_key::record,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportedLicenseDto {
    course_id: String,
    course_title: String,
    versions: Vec<String>,
    unlocks_current: bool,
    already_imported: bool,
}

impl From<ImportedLicense> for ImportedLicenseDto {
    fn from(license: ImportedLicense) -> Self {
        Self {
            course_id: license.course_id,
            course_title: license.course_title,
            versions: license.versions,
            unlocks_current: license.unlocks_current,
            already_imported: !license.new,
        }
    }
}

/// Choose a license file in a native dialog and import it. Returns `None`
/// when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn choose_and_import_license(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ImportedLicenseDto>, CommandError> {
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron license", &["json"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| CommandError {
        code: "unsupported_file_location",
        message: "the selected file location is not supported".to_owned(),
    })?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let bytes = read_bounded(&source)?;
        import(&mut *lock_library(&library)?, &bytes).map(Some)
    })
    .await
}

fn import(library: &mut CourseLibrary, bytes: &[u8]) -> Result<ImportedLicenseDto, CommandError> {
    let learner = record(library)?.ok_or_else(|| CommandError {
        code: "learner_key_missing",
        message: "create your learner key in Settings first; licenses are made for it".to_owned(),
    })?;
    let learner_key: PublicKey = learner.signing_key.parse().map_err(|_| CommandError {
        code: "learner_key_missing",
        message: "the recorded learner key is damaged; restore it from a backup".to_owned(),
    })?;
    library
        .import_license(bytes, &learner_key)
        .map(ImportedLicenseDto::from)
        .map_err(|error| match error {
            LibraryError::License(_) => command_error("license_invalid", error),
            // Storage errors can name the database location; keep them in Rust.
            _ => CommandError {
                code: "library_unavailable",
                message: "the license could not be saved".to_owned(),
            },
        })
}

fn read_bounded(source: &std::path::Path) -> Result<Vec<u8>, CommandError> {
    use std::io::Read as _;
    let unreadable = || CommandError {
        code: "license_unreadable",
        message: "the chosen file could not be read".to_owned(),
    };
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .map_err(|_| unreadable())?
        .take(MAX_LICENSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use learning_application::ImportLimits;

    use super::*;

    #[test]
    fn importing_needs_a_learner_key_and_refuses_bad_files() {
        let mut library = CourseLibrary::open_in_memory(ImportLimits::default()).unwrap();
        assert_eq!(
            import(&mut library, b"{}").unwrap_err().code,
            "learner_key_missing"
        );
        let learner =
            learner_identity::create(&learner_identity::MemoryStore::default(), 1).unwrap();
        library
            .set_learner_identity_record(&serde_json::to_string(&learner).unwrap())
            .unwrap();
        let refused = import(&mut library, b"not a license").unwrap_err();
        assert_eq!(refused.code, "license_invalid");
        assert!(!refused.message.contains('/'));
    }
}
