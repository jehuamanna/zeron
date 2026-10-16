//! Licensing a learner by hand from Publish (ADR 0017): the publisher pastes
//! the learner ID the learner shares, and saves a license file that unlocks
//! every released paid version of the course. No issuer and no network.

use std::sync::Arc;

use course_signing::LicenseFile;
use learner_identity::parse_learner_id;
use publisher_application::{PublisherDrafts, issue_license};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    PublishState,
    commands::{CommandError, lock_drafts, run_blocking, service_error},
    identity::{Credentials, publisher_signer},
};

/// Make a license for `learner_id` and save it where the publisher chooses.
/// Returns `false` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn issue_learner_license(
    app: AppHandle,
    course_id: String,
    learner_id: String,
    state: State<'_, PublishState>,
) -> Result<bool, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    let file_name = format!("zeron-license-{course_id}.json");
    let license = run_blocking(move || {
        license(
            &*lock_drafts(&drafts)?,
            &credentials,
            &course_id,
            &learner_id,
            now(),
        )
    })
    .await?
    .to_bytes();
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name(&file_name)
            .add_filter("Zeron license", &["json"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let destination = selected.into_path().map_err(|_| {
        CommandError::new(
            "unsupported_file_location",
            "the selected file location is not supported",
        )
    })?;
    run_blocking(move || {
        std::fs::write(&destination, license).map_err(|_| {
            CommandError::new(
                "export_failed",
                "the license could not be written to the chosen location",
            )
        })
    })
    .await?;
    Ok(true)
}

fn license(
    drafts: &PublisherDrafts,
    credentials: &Credentials,
    course_id: &str,
    learner_id: &str,
    now: u64,
) -> Result<LicenseFile, CommandError> {
    let learner = parse_learner_id(learner_id).map_err(|_| {
        CommandError::new(
            "learner_id_invalid",
            "this is not a learner ID; ask the learner to copy it from Settings → Learner key",
        )
    })?;
    let signer = publisher_signer(drafts, credentials, now)?;
    let versions = drafts
        .released_versions(course_id)
        .map_err(|error| service_error("license_failed", error))?;
    let store = credentials.store()?;
    issue_license(
        store.as_ref(),
        &signer,
        course_id,
        &versions,
        &learner.signing_key,
        &learner.encryption_key,
    )
    .map_err(|error| service_error("license_failed", error))
}

fn now() -> u64 {
    super::tracker_lists::now_unix_seconds()
}

#[cfg(test)]
mod tests {
    use learner_identity::MemoryStore;

    use super::*;

    #[test]
    fn a_license_needs_an_identity_a_learner_id_and_a_paid_release() {
        let drafts = PublisherDrafts::open_in_memory().unwrap();
        let store = Arc::new(MemoryStore::default());
        let credentials =
            Credentials::with(Arc::clone(&store) as Arc<dyn publisher_identity::CredentialStore>);
        let learner = learner_identity::create(&MemoryStore::default(), 1).unwrap();
        let learner_id = learner.learner_id();
        assert_eq!(
            license(&drafts, &credentials, "c", "nonsense", 1)
                .unwrap_err()
                .code(),
            "learner_id_invalid"
        );
        assert_eq!(
            license(&drafts, &credentials, "c", &learner_id, 1)
                .unwrap_err()
                .code(),
            "identity_missing"
        );
        let mut drafts = drafts;
        let identity = publisher_identity::create(store.as_ref(), 1).unwrap();
        drafts.set_identity(&identity).unwrap();
        // No release of the course has a content key on this computer.
        assert_eq!(
            license(&drafts, &credentials, "c", &learner_id, 1)
                .unwrap_err()
                .code(),
            "license_failed"
        );
    }
}
