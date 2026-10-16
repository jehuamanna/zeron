//! The publisher's catalog: which releases it lists, key revocations, and
//! exporting the signed catalog file through a native save dialog.

use std::sync::Arc;

use course_signing::fingerprint;
use publisher_application::PublisherDrafts;
use publisher_identity::{Identity, signing_key};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    PublishState,
    commands::{CommandError, ReleaseDto, lock_drafts, run_blocking, service_error},
    identity::Credentials,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogDto {
    title: String,
    listings: Vec<CatalogListingDto>,
    revocations: Vec<RevocationDto>,
    /// Keys this identity retired and has not revoked.
    retired_keys: Vec<RetiredKeyDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogListingDto {
    release: ReleaseDto,
    listed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevocationDto {
    revoked_key: String,
    fingerprint: String,
    revoked_at_unix_seconds: u64,
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetiredKeyDto {
    key: String,
    fingerprint: String,
    retired_at_unix_seconds: u64,
}

/// The catalog as it would be exported.
#[tauri::command]
pub(crate) async fn get_catalog(
    state: State<'_, PublishState>,
) -> Result<CatalogDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || describe(&*lock_drafts(&drafts)?)).await
}

/// Set the title learners see.
#[tauri::command]
pub(crate) async fn set_catalog_title(
    title: String,
    state: State<'_, PublishState>,
) -> Result<CatalogDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        drafts
            .set_catalog_title(&title)
            .map_err(|error| service_error("catalog_failed", error))?;
        describe(&drafts)
    })
    .await
}

/// Confirm a release for the catalog, or take it out. Listing is always an
/// explicit choice per release.
#[tauri::command]
pub(crate) async fn set_catalog_release(
    release_id: String,
    listed: bool,
    state: State<'_, PublishState>,
) -> Result<CatalogDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        drafts
            .set_catalog_release(&release_id, listed)
            .map_err(|error| service_error("catalog_failed", error))?;
        describe(&drafts)
    })
    .await
}

/// Revoke a retired key from the moment it was retired. Catalogs carry the
/// revocation to learners.
#[tauri::command]
pub(crate) async fn revoke_retired_key(
    key: String,
    reason: String,
    state: State<'_, PublishState>,
) -> Result<CatalogDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        revoke(&mut drafts, &credentials, &key, &reason)?;
        describe(&drafts)
    })
    .await
}

/// Save the signed catalog where the publisher chooses. Returns `false`
/// when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_catalog(
    app: AppHandle,
    state: State<'_, PublishState>,
) -> Result<bool, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    let releases_root = state.releases_root.clone();
    let bytes =
        run_blocking(move || build(&*lock_drafts(&drafts)?, &credentials, &releases_root)).await?;
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name("zeron-catalog.json")
            .add_filter("Zeron catalog", &["json"])
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
        std::fs::write(&destination, bytes).map_err(|_| {
            CommandError::new(
                "export_failed",
                "the catalog could not be written to the chosen location",
            )
        })
    })
    .await?;
    Ok(true)
}

fn describe(drafts: &PublisherDrafts) -> Result<CatalogDto, CommandError> {
    let failed = |error| service_error("catalog_failed", error);
    let revocations = drafts.issued_revocations().map_err(failed)?;
    let identity = drafts.identity().map_err(failed)?;
    let retired_keys = identity
        .iter()
        .flat_map(|identity| &identity.rotations)
        .filter(|rotation| {
            !revocations.iter().any(|revocation| {
                revocation.statement.revoked_key == rotation.statement.previous_key
            })
        })
        .map(|rotation| RetiredKeyDto {
            fingerprint: key_fingerprint(&rotation.statement.previous_key),
            key: rotation.statement.previous_key.clone(),
            retired_at_unix_seconds: rotation.statement.rotated_at,
        })
        .collect();
    Ok(CatalogDto {
        title: drafts.catalog_title().map_err(failed)?,
        listings: drafts
            .catalog_listings()
            .map_err(failed)?
            .into_iter()
            .map(|listing| CatalogListingDto {
                release: ReleaseDto::from(listing.release),
                listed: listing.listed,
            })
            .collect(),
        revocations: revocations
            .into_iter()
            .map(|revocation| RevocationDto {
                fingerprint: key_fingerprint(&revocation.statement.revoked_key),
                revoked_key: revocation.statement.revoked_key,
                revoked_at_unix_seconds: revocation.statement.revoked_at,
                reason: revocation.statement.reason,
            })
            .collect(),
        retired_keys,
    })
}

fn revoke(
    drafts: &mut PublisherDrafts,
    credentials: &Credentials,
    key: &str,
    reason: &str,
) -> Result<(), CommandError> {
    let identity = required_identity(drafts)?;
    let retired_at = identity
        .rotations
        .iter()
        .find(|rotation| rotation.statement.previous_key == key)
        .map(|rotation| rotation.statement.rotated_at)
        .ok_or_else(|| {
            CommandError::new(
                "invalid_input",
                "this key is not a retired key of your identity",
            )
        })?;
    let store = credentials.store()?;
    let secret = signing_key(store.as_ref(), &identity)
        .map_err(|error| CommandError::new("key_missing", error))?;
    drafts
        .revoke_retired_key(&identity, &secret, key, retired_at, reason)
        .map_err(|error| service_error("catalog_failed", error))?;
    Ok(())
}

fn build(
    drafts: &PublisherDrafts,
    credentials: &Credentials,
    releases_root: &std::path::Path,
) -> Result<Vec<u8>, CommandError> {
    let identity = required_identity(drafts)?;
    let store = credentials.store()?;
    let secret = signing_key(store.as_ref(), &identity)
        .map_err(|error| CommandError::new("key_missing", error))?;
    drafts
        .build_catalog(
            releases_root,
            &identity,
            &secret,
            super::tracker_lists::now_unix_seconds(),
        )
        .map(|catalog| catalog.to_bytes())
        .map_err(|error| service_error("catalog_failed", error))
}

fn required_identity(drafts: &PublisherDrafts) -> Result<Identity, CommandError> {
    drafts
        .identity()
        .map_err(|error| service_error("identity_failed", error))?
        .ok_or_else(|| {
            CommandError::new(
                "identity_missing",
                "create a signing identity before publishing a catalog",
            )
        })
}

fn key_fingerprint(key: &str) -> String {
    key.parse()
        .map_or_else(|_| String::new(), |key| fingerprint(&key))
}

#[cfg(test)]
mod tests {
    use publisher_identity::{MemoryStore, create, rotate_key};

    use super::*;

    #[test]
    fn the_catalog_needs_an_identity_and_revokes_only_retired_keys() {
        let store: Arc<dyn publisher_identity::CredentialStore> = Arc::new(MemoryStore::default());
        let credentials = Credentials::with(Arc::clone(&store));
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let releases =
            std::env::temp_dir().join(format!("zeron-catalog-cmd-{}", std::process::id()));
        assert_eq!(
            build(&drafts, &credentials, &releases).unwrap_err().code(),
            "identity_missing"
        );
        let first = create(store.as_ref(), 1).unwrap();
        let identity = rotate_key(store.as_ref(), &first, 7).unwrap();
        drafts.set_identity(&identity).unwrap();
        let described = serde_json::to_value(describe(&drafts).unwrap()).unwrap();
        assert_eq!(described["title"], "My courses");
        assert_eq!(described["retiredKeys"][0]["key"], first.current.as_str());
        assert_eq!(described["retiredKeys"][0]["retiredAtUnixSeconds"], 7);

        assert_eq!(
            revoke(&mut drafts, &credentials, &identity.current, "x")
                .unwrap_err()
                .code(),
            "invalid_input"
        );
        revoke(&mut drafts, &credentials, &first.current, "old laptop").unwrap();
        let after = serde_json::to_value(describe(&drafts).unwrap()).unwrap();
        assert_eq!(after["retiredKeys"].as_array().unwrap().len(), 0);
        assert_eq!(after["revocations"][0]["revokedAtUnixSeconds"], 7);
        assert_eq!(after["revocations"][0]["reason"], "old laptop");

        let bytes = build(&drafts, &credentials, &releases).unwrap();
        let catalog = course_signing::parse_catalog(&bytes).unwrap();
        course_signing::verify_catalog(&catalog).unwrap();
        assert_eq!(catalog.statement.revocations.len(), 1);
        let text = String::from_utf8(bytes).unwrap();
        let secret = signing_key(store.as_ref(), &identity).unwrap();
        let secret_hex = secret
            .to_bytes()
            .iter()
            .fold(String::new(), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        assert!(!text.contains(&secret_hex));
    }
}
