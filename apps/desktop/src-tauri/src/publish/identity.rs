//! The publisher's signing identity: create, rotate, back up, and restore
//! (ADR 0014). Secret keys stay in the OS credential store; only public keys,
//! fingerprints, and statuses cross IPC. Backup locations come from native
//! dialogs and stay in Rust.

use std::sync::{Arc, OnceLock};

use publisher_application::{PublisherDrafts, ReleaseSigner};
use publisher_identity::{
    CredentialStore, Identity, IdentityError, MAX_BACKUP_BYTES, OsStore, create, export_backup,
    forget_key, restore_backup, rotate_key, signing_key,
};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use super::{
    PublishState,
    commands::{CommandError, lock_drafts, run_blocking, service_error},
};

/// The credential store, opened on first use so a missing keychain never
/// delays or blocks startup.
#[derive(Default)]
pub(crate) struct Credentials {
    store: OnceLock<Result<Arc<dyn CredentialStore>, String>>,
}

impl Credentials {
    /// Use `store` instead of the OS credential store, for tests.
    #[cfg(test)]
    pub(crate) fn with(store: Arc<dyn CredentialStore>) -> Self {
        let credentials = Self::default();
        let _ = credentials.store.set(Ok(store));
        credentials
    }

    fn get(&self) -> Result<Arc<dyn CredentialStore>, CommandError> {
        self.store
            .get_or_init(|| {
                OsStore::open()
                    .map(|store| Arc::new(store) as Arc<dyn CredentialStore>)
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(|message| CommandError::new("keychain_unavailable", message))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdentityDto {
    /// `none`, `ready`, `key_missing`, or `keychain_unavailable`.
    status: &'static str,
    publisher_id: Option<String>,
    signing_key: Option<String>,
    fingerprint: Option<String>,
    created_at_unix_seconds: Option<u64>,
    rotations: usize,
    sign_releases: bool,
    /// Why the keychain or key cannot be used, when it cannot.
    problem: Option<String>,
}

/// The identity and whether this computer can sign with it.
#[tauri::command]
pub(crate) async fn get_publisher_identity(
    state: State<'_, PublishState>,
) -> Result<IdentityDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || describe(&*lock_drafts(&drafts)?, &credentials)).await
}

/// Create the publisher's identity with a new key in the OS keychain.
#[tauri::command]
pub(crate) async fn create_publisher_identity(
    state: State<'_, PublishState>,
) -> Result<IdentityDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        create_identity(&mut drafts, &credentials)?;
        describe(&drafts, &credentials)
    })
    .await
}

/// Retire the current key and sign with a new one from now on. Learners
/// keep trusting the same identity through the signed rotation.
#[tauri::command]
pub(crate) async fn rotate_publisher_identity(
    state: State<'_, PublishState>,
) -> Result<IdentityDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        rotate_identity(&mut drafts, &credentials)?;
        describe(&drafts, &credentials)
    })
    .await
}

/// Sign new releases, or stop signing them.
#[tauri::command]
pub(crate) async fn set_sign_releases(
    enabled: bool,
    state: State<'_, PublishState>,
) -> Result<IdentityDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || {
        let mut drafts = lock_drafts(&drafts)?;
        drafts
            .set_sign_releases(enabled)
            .map_err(|error| service_error("settings_failed", error))?;
        describe(&drafts, &credentials)
    })
    .await
}

/// Save a passphrase-encrypted backup of the signing key where the
/// publisher chooses. Returns `false` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_identity_backup(
    app: AppHandle,
    passphrase: String,
    state: State<'_, PublishState>,
) -> Result<bool, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    let backup =
        run_blocking(move || backup(&*lock_drafts(&drafts)?, &credentials, &passphrase)).await?;
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name("zeron-publisher-key.json")
            .add_filter("Zeron publisher key backup", &["json"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let destination = selected.into_path().map_err(|_| unsupported_location())?;
    run_blocking(move || {
        std::fs::write(&destination, backup).map_err(|_| {
            CommandError::new(
                "export_failed",
                "the backup could not be written to the chosen location",
            )
        })
    })
    .await?;
    Ok(true)
}

/// Restore the signing key from a backup chosen in a native dialog. Returns
/// `None` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn restore_identity_backup(
    app: AppHandle,
    passphrase: String,
    state: State<'_, PublishState>,
) -> Result<Option<IdentityDto>, CommandError> {
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron publisher key backup", &["json"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| unsupported_location())?;
    let drafts = Arc::clone(&state.drafts);
    let credentials = Arc::clone(&state.credentials);
    run_blocking(move || {
        let bytes = read_bounded(&source)?;
        let mut drafts = lock_drafts(&drafts)?;
        restore(&mut drafts, &credentials, &bytes, &passphrase)?;
        describe(&drafts, &credentials).map(Some)
    })
    .await
}

/// The signer for a new preparation: `None` when there is no identity or the
/// publisher turned signing off. A failure to reach the key stops the
/// preparation rather than publishing an unsigned release by surprise.
pub(crate) fn release_signer(
    drafts: &PublisherDrafts,
    credentials: &Credentials,
    now: u64,
) -> Result<Option<ReleaseSigner>, CommandError> {
    let Some(identity) = identity(drafts)? else {
        return Ok(None);
    };
    if !drafts
        .sign_releases()
        .map_err(|error| service_error("settings_failed", error))?
    {
        return Ok(None);
    }
    let store = credentials.get()?;
    let secret = signing_key(store.as_ref(), &identity).map_err(identity_error)?;
    ReleaseSigner::new(identity, secret, now)
        .map(Some)
        .map_err(|error| service_error("signing_unavailable", error))
}

fn describe(
    drafts: &PublisherDrafts,
    credentials: &Credentials,
) -> Result<IdentityDto, CommandError> {
    let sign_releases = drafts
        .sign_releases()
        .map_err(|error| service_error("settings_failed", error))?;
    let Some(identity) = identity(drafts)? else {
        return Ok(IdentityDto {
            status: "none",
            publisher_id: None,
            signing_key: None,
            fingerprint: None,
            created_at_unix_seconds: None,
            rotations: 0,
            sign_releases,
            problem: None,
        });
    };
    let (status, problem) = match credentials
        .get()
        .and_then(|store| signing_key(store.as_ref(), &identity).map_err(identity_error))
    {
        Ok(_) => ("ready", None),
        Err(error) if error.code() == "keychain_unavailable" => {
            ("keychain_unavailable", Some(error.message().to_owned()))
        }
        Err(error) => ("key_missing", Some(error.message().to_owned())),
    };
    Ok(IdentityDto {
        status,
        fingerprint: identity.fingerprint().ok(),
        publisher_id: Some(identity.root.clone()),
        signing_key: Some(identity.current.clone()),
        created_at_unix_seconds: Some(identity.created_at),
        rotations: identity.rotations.len(),
        sign_releases,
        problem,
    })
}

fn create_identity(
    drafts: &mut PublisherDrafts,
    credentials: &Credentials,
) -> Result<Identity, CommandError> {
    if identity(drafts)?.is_some() {
        return Err(CommandError::new(
            "identity_exists",
            "this publisher already has a signing identity",
        ));
    }
    let store = credentials.get()?;
    let identity = create(store.as_ref(), now()).map_err(identity_error)?;
    drafts
        .set_identity(&identity)
        .map_err(|error| service_error("identity_failed", error))?;
    Ok(identity)
}

fn rotate_identity(
    drafts: &mut PublisherDrafts,
    credentials: &Credentials,
) -> Result<Identity, CommandError> {
    let current = identity(drafts)?
        .ok_or_else(|| CommandError::new("identity_missing", "create a signing identity first"))?;
    let store = credentials.get()?;
    let rotated = rotate_key(store.as_ref(), &current, now()).map_err(identity_error)?;
    drafts
        .set_identity(&rotated)
        .map_err(|error| service_error("identity_failed", error))?;
    // The rotation is recorded; the retired key is no longer needed.
    if let Ok(retired) = current.current_key() {
        let _ = forget_key(store.as_ref(), &retired);
    }
    Ok(rotated)
}

fn backup(
    drafts: &PublisherDrafts,
    credentials: &Credentials,
    passphrase: &str,
) -> Result<Vec<u8>, CommandError> {
    let identity = identity(drafts)?
        .ok_or_else(|| CommandError::new("identity_missing", "create a signing identity first"))?;
    let store = credentials.get()?;
    export_backup(store.as_ref(), &identity, passphrase).map_err(identity_error)
}

fn restore(
    drafts: &mut PublisherDrafts,
    credentials: &Credentials,
    bytes: &[u8],
    passphrase: &str,
) -> Result<Identity, CommandError> {
    let store = credentials.get()?;
    let existing = identity(drafts)?;
    let restored = restore_backup(store.as_ref(), bytes, passphrase).map_err(identity_error)?;
    if let Some(existing) = existing.filter(|existing| existing.root != restored.root) {
        let _ = forget_key(
            store.as_ref(),
            &restored.current_key().map_err(identity_error)?,
        );
        return Err(CommandError::new(
            "identity_exists",
            format!(
                "this backup belongs to another publisher identity than {}",
                existing.fingerprint().unwrap_or_default()
            ),
        ));
    }
    drafts
        .set_identity(&restored)
        .map_err(|error| service_error("identity_failed", error))?;
    Ok(restored)
}

fn identity(drafts: &PublisherDrafts) -> Result<Option<Identity>, CommandError> {
    drafts
        .identity()
        .map_err(|error| service_error("identity_failed", error))
}

fn identity_error(error: IdentityError) -> CommandError {
    let code = match error {
        IdentityError::Store(_) => "keychain_unavailable",
        IdentityError::MissingKey | IdentityError::WrongKey => "key_missing",
        IdentityError::InvalidBackup => "backup_invalid",
        IdentityError::WrongPassphrase => "wrong_passphrase",
        IdentityError::WeakPassphrase => "weak_passphrase",
        IdentityError::Random(_) | IdentityError::Signing(_) => "identity_failed",
    };
    CommandError::new(code, error)
}

fn read_bounded(source: &std::path::Path) -> Result<Vec<u8>, CommandError> {
    use std::io::Read as _;
    let unreadable = || CommandError::new("backup_unreadable", "the chosen file could not be read");
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .map_err(|_| unreadable())?
        .take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

fn unsupported_location() -> CommandError {
    CommandError::new(
        "unsupported_file_location",
        "the selected file location is not supported",
    )
}

fn now() -> u64 {
    super::tracker_lists::now_unix_seconds()
}

#[cfg(test)]
mod tests {
    use publisher_identity::MemoryStore;

    use super::*;

    fn fixture() -> (PublisherDrafts, Credentials) {
        (
            PublisherDrafts::open_in_memory().unwrap(),
            Credentials::with(Arc::new(MemoryStore::default())),
        )
    }

    #[test]
    fn identities_are_created_once_rotated_and_described_without_secrets() {
        let (mut drafts, credentials) = fixture();
        let none = serde_json::to_value(describe(&drafts, &credentials).unwrap()).unwrap();
        assert_eq!(none["status"], "none");
        assert_eq!(none["signReleases"], true);
        assert!(release_signer(&drafts, &credentials, 1).unwrap().is_none());

        let created = create_identity(&mut drafts, &credentials).unwrap();
        assert_eq!(
            create_identity(&mut drafts, &credentials)
                .unwrap_err()
                .code(),
            "identity_exists"
        );
        let ready = serde_json::to_value(describe(&drafts, &credentials).unwrap()).unwrap();
        assert_eq!(ready["status"], "ready");
        assert_eq!(ready["publisherId"], created.root.as_str());
        assert_eq!(ready["fingerprint"].as_str().unwrap().len(), 19);
        let secret = signing_key(credentials.get().unwrap().as_ref(), &created).unwrap();
        let secret_hex = secret
            .to_bytes()
            .iter()
            .fold(String::new(), |mut hex, byte| {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        assert!(!ready.to_string().contains(&secret_hex));
        assert!(release_signer(&drafts, &credentials, 1).unwrap().is_some());

        let rotated = rotate_identity(&mut drafts, &credentials).unwrap();
        assert_eq!(rotated.root, created.root);
        let after = serde_json::to_value(describe(&drafts, &credentials).unwrap()).unwrap();
        assert_eq!(after["rotations"], 1);
        assert_eq!(after["status"], "ready");
        assert!(matches!(
            signing_key(credentials.get().unwrap().as_ref(), &created),
            Err(IdentityError::MissingKey)
        ));

        drafts.set_sign_releases(false).unwrap();
        assert!(release_signer(&drafts, &credentials, 1).unwrap().is_none());
    }

    #[test]
    fn backups_restore_the_same_identity_and_refuse_another() {
        let (mut drafts, credentials) = fixture();
        let identity = create_identity(&mut drafts, &credentials).unwrap();
        assert_eq!(
            backup(&drafts, &credentials, "short").unwrap_err().code(),
            "weak_passphrase"
        );
        let saved = backup(&drafts, &credentials, "a long passphrase here").unwrap();

        let (mut elsewhere, empty) = fixture();
        let missing = serde_json::to_value(describe(&elsewhere, &empty).unwrap()).unwrap();
        assert_eq!(missing["status"], "none");
        assert_eq!(
            restore(&mut elsewhere, &empty, &saved, "wrong passphrase!!")
                .unwrap_err()
                .code(),
            "wrong_passphrase"
        );
        assert_eq!(
            restore(&mut elsewhere, &empty, &saved, "a long passphrase here")
                .unwrap()
                .root,
            identity.root
        );
        assert_eq!(
            serde_json::to_value(describe(&elsewhere, &empty).unwrap()).unwrap()["status"],
            "ready"
        );

        let (mut other, other_store) = fixture();
        create_identity(&mut other, &other_store).unwrap();
        assert_eq!(
            restore(&mut other, &other_store, &saved, "a long passphrase here")
                .unwrap_err()
                .code(),
            "identity_exists"
        );
        assert_eq!(
            restore(&mut other, &other_store, b"{}", "x")
                .unwrap_err()
                .code(),
            "backup_invalid"
        );

        let (mut lost, lost_store) = fixture();
        lost.set_identity(&identity).unwrap();
        let described = serde_json::to_value(describe(&lost, &lost_store).unwrap()).unwrap();
        assert_eq!(described["status"], "key_missing");
        assert_eq!(
            release_signer(&lost, &lost_store, 1).unwrap_err().code(),
            "key_missing"
        );
    }
}
