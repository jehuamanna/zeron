//! This computer's learner key (ADR 0017 decision 3): create it, see its
//! fingerprint, and back it up or restore it with a passphrase. The secret
//! stays in the OS keychain; only public keys, the fingerprint, and a status
//! cross IPC. Backup locations come from native dialogs and stay in Rust.

use std::sync::{Arc, Mutex, OnceLock};

use learner_identity::{
    CredentialStore, LearnerIdentity, LearnerKeyError, MAX_BACKUP_BYTES, OsStore, SERVICE, create,
    export_backup, learner_keys, restore_backup, stored_identity,
};
use learning_application::CourseLibrary;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    AppState,
    commands::{CommandError, run_blocking},
};

/// The learner key's credential store, opened on first use so a missing
/// keychain never delays startup.
#[derive(Default)]
pub(crate) struct LearnerCredentials {
    store: OnceLock<Result<Arc<dyn CredentialStore>, String>>,
}

impl LearnerCredentials {
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
                OsStore::open_service(SERVICE)
                    .map(|store| Arc::new(store) as Arc<dyn CredentialStore>)
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(|message| CommandError {
                code: "keychain_unavailable",
                message,
            })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LearnerKeyDto {
    /// `none`, `ready`, `key_missing`, or `keychain_unavailable`.
    status: &'static str,
    signing_key: Option<String>,
    encryption_key: Option<String>,
    fingerprint: Option<String>,
    created_at_unix_seconds: Option<u64>,
    /// Why the keychain or key cannot be used, when it cannot.
    problem: Option<String>,
}

/// The learner key and whether this computer holds its secret.
#[tauri::command]
pub(crate) async fn get_learner_key(
    state: State<'_, AppState>,
) -> Result<LearnerKeyDto, CommandError> {
    let library = Arc::clone(&state.library);
    let credentials = Arc::clone(&state.learner_credentials);
    run_blocking(move || describe(&library, &credentials)).await
}

/// Create this computer's learner key in the OS keychain.
#[tauri::command]
pub(crate) async fn create_learner_key(
    state: State<'_, AppState>,
) -> Result<LearnerKeyDto, CommandError> {
    let library = Arc::clone(&state.library);
    let credentials = Arc::clone(&state.learner_credentials);
    run_blocking(move || {
        create_key(&library, &credentials)?;
        describe(&library, &credentials)
    })
    .await
}

/// Save a passphrase-encrypted backup of the learner key where the learner
/// chooses. Returns `false` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_learner_key_backup(
    app: AppHandle,
    passphrase: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    let library = Arc::clone(&state.library);
    let credentials = Arc::clone(&state.learner_credentials);
    let backup = run_blocking(move || backup(&library, &credentials, &passphrase)).await?;
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name("zeron-learner-key.json")
            .add_filter("Zeron learner key backup", &["json"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let destination = selected.into_path().map_err(|_| unsupported_location())?;
    run_blocking(move || {
        std::fs::write(&destination, backup).map_err(|_| CommandError {
            code: "export_failed",
            message: "the backup could not be written to the chosen location".to_owned(),
        })
    })
    .await?;
    Ok(true)
}

/// Restore the learner key from a backup chosen in a native dialog. Returns
/// `None` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn restore_learner_key_backup(
    app: AppHandle,
    passphrase: String,
    state: State<'_, AppState>,
) -> Result<Option<LearnerKeyDto>, CommandError> {
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .add_filter("Zeron learner key backup", &["json"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| unsupported_location())?;
    let library = Arc::clone(&state.library);
    let credentials = Arc::clone(&state.learner_credentials);
    run_blocking(move || {
        let bytes = read_bounded(&source)?;
        restore(&library, &credentials, &bytes, &passphrase)?;
        describe(&library, &credentials).map(Some)
    })
    .await
}

fn lock(
    library: &Mutex<CourseLibrary>,
) -> Result<std::sync::MutexGuard<'_, CourseLibrary>, CommandError> {
    library.lock().map_err(|_| library_failure())
}

fn record(library: &CourseLibrary) -> Result<Option<LearnerIdentity>, CommandError> {
    let json = library
        .learner_identity_record()
        .map_err(|_| library_failure())?;
    // A damaged record is treated as none; the keychain still holds the key.
    Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
}

fn save_record(
    library: &mut CourseLibrary,
    identity: &LearnerIdentity,
) -> Result<(), CommandError> {
    let json = serde_json::to_string(identity).map_err(|_| library_failure())?;
    library
        .set_learner_identity_record(&json)
        .map_err(|_| library_failure())
}

/// The key's status. A key the keychain holds without a Library record, for
/// example after the database was reset, is recorded again.
fn describe(
    library: &Mutex<CourseLibrary>,
    credentials: &LearnerCredentials,
) -> Result<LearnerKeyDto, CommandError> {
    let mut library = lock(library)?;
    let recorded = record(&library)?;
    let store = match credentials.get() {
        Ok(store) => store,
        Err(error) => {
            return Ok(dto(
                "keychain_unavailable",
                recorded.as_ref(),
                Some(error.message),
            ));
        }
    };
    let Some(identity) = recorded else {
        return match stored_identity(store.as_ref(), now()) {
            Ok(Some(found)) => {
                save_record(&mut library, &found)?;
                Ok(dto("ready", Some(&found), None))
            }
            Ok(None) => Ok(dto("none", None, None)),
            Err(error) => Ok(problem(&error, None)),
        };
    };
    Ok(match learner_keys(store.as_ref(), &identity) {
        Ok(_) => dto("ready", Some(&identity), None),
        Err(error) => problem(&error, Some(&identity)),
    })
}

fn problem(error: &LearnerKeyError, identity: Option<&LearnerIdentity>) -> LearnerKeyDto {
    let status = match error {
        LearnerKeyError::Store(_) => "keychain_unavailable",
        _ => "key_missing",
    };
    dto(status, identity, Some(error.to_string()))
}

fn dto(
    status: &'static str,
    identity: Option<&LearnerIdentity>,
    problem: Option<String>,
) -> LearnerKeyDto {
    LearnerKeyDto {
        status,
        signing_key: identity.map(|identity| identity.signing_key.clone()),
        encryption_key: identity.map(|identity| identity.encryption_key.clone()),
        fingerprint: identity.and_then(|identity| identity.fingerprint().ok()),
        created_at_unix_seconds: identity.map(|identity| identity.created_at),
        problem,
    }
}

fn create_key(
    library: &Mutex<CourseLibrary>,
    credentials: &LearnerCredentials,
) -> Result<LearnerIdentity, CommandError> {
    let mut library = lock(library)?;
    if record(&library)?.is_some() {
        return Err(CommandError {
            code: "learner_key_exists",
            message: "this computer already has a learner key".to_owned(),
        });
    }
    let store = credentials.get()?;
    let identity = match create(store.as_ref(), now()) {
        // The keychain kept a key the Library lost track of: use it.
        Err(LearnerKeyError::Exists) => stored_identity(store.as_ref(), now())
            .map_err(|error| learner_error(&error))?
            .ok_or_else(|| learner_error(&LearnerKeyError::Missing))?,
        other => other.map_err(|error| learner_error(&error))?,
    };
    save_record(&mut library, &identity)?;
    Ok(identity)
}

fn backup(
    library: &Mutex<CourseLibrary>,
    credentials: &LearnerCredentials,
    passphrase: &str,
) -> Result<Vec<u8>, CommandError> {
    let identity = record(&*lock(library)?)?.ok_or_else(|| CommandError {
        code: "learner_key_missing",
        message: "create a learner key first".to_owned(),
    })?;
    let store = credentials.get()?;
    export_backup(store.as_ref(), &identity, passphrase).map_err(|error| learner_error(&error))
}

fn restore(
    library: &Mutex<CourseLibrary>,
    credentials: &LearnerCredentials,
    bytes: &[u8],
    passphrase: &str,
) -> Result<LearnerIdentity, CommandError> {
    let store = credentials.get()?;
    let identity =
        restore_backup(store.as_ref(), bytes, passphrase).map_err(|error| learner_error(&error))?;
    save_record(&mut *lock(library)?, &identity)?;
    Ok(identity)
}

fn learner_error(error: &LearnerKeyError) -> CommandError {
    let code = match error {
        LearnerKeyError::Store(_) => "keychain_unavailable",
        LearnerKeyError::Missing | LearnerKeyError::WrongKey => "key_missing",
        LearnerKeyError::Exists => "learner_key_exists",
        LearnerKeyError::InvalidBackup => "backup_invalid",
        LearnerKeyError::WrongPassphrase => "wrong_passphrase",
        LearnerKeyError::WeakPassphrase => "weak_passphrase",
        LearnerKeyError::Random(_) => "learner_key_failed",
    };
    CommandError {
        code,
        message: error.to_string(),
    }
}

fn library_failure() -> CommandError {
    // Storage errors can name the database location; keep them in Rust.
    CommandError {
        code: "library_unavailable",
        message: "the library could not be read".to_owned(),
    }
}

fn read_bounded(source: &std::path::Path) -> Result<Vec<u8>, CommandError> {
    use std::io::Read as _;
    let unreadable = || CommandError {
        code: "backup_unreadable",
        message: "the chosen file could not be read".to_owned(),
    };
    let mut bytes = Vec::new();
    std::fs::File::open(source)
        .map_err(|_| unreadable())?
        .take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

fn unsupported_location() -> CommandError {
    CommandError {
        code: "unsupported_file_location",
        message: "the selected file location is not supported".to_owned(),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use learner_identity::{ACCOUNT, MemoryStore};
    use learning_application::ImportLimits;

    use super::*;

    const PASSPHRASE: &str = "correct horse battery";

    fn fixture() -> (Mutex<CourseLibrary>, LearnerCredentials, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        (
            Mutex::new(CourseLibrary::open_in_memory(ImportLimits::default()).unwrap()),
            LearnerCredentials::with(Arc::clone(&store) as Arc<dyn CredentialStore>),
            store,
        )
    }

    fn json(dto: &LearnerKeyDto) -> serde_json::Value {
        serde_json::to_value(dto).unwrap()
    }

    #[test]
    fn a_learner_key_is_created_once_and_described_without_secrets() {
        let (library, credentials, store) = fixture();
        assert_eq!(
            json(&describe(&library, &credentials).unwrap())["status"],
            "none"
        );
        let created = create_key(&library, &credentials).unwrap();
        assert_eq!(
            create_key(&library, &credentials).unwrap_err().code,
            "learner_key_exists"
        );
        let ready = json(&describe(&library, &credentials).unwrap());
        assert_eq!(ready["status"], "ready");
        assert_eq!(ready["signingKey"], created.signing_key.as_str());
        assert_eq!(ready["encryptionKey"], created.encryption_key.as_str());
        assert_eq!(ready["fingerprint"].as_str().unwrap().len(), 19);
        let seed = store.load(ACCOUNT).unwrap().unwrap();
        let seed_hex = seed.iter().fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        assert!(!ready.to_string().contains(&seed_hex));
    }

    #[test]
    fn a_lost_record_is_recovered_and_a_lost_key_is_reported() {
        let (library, credentials, store) = fixture();
        let created = create_key(&library, &credentials).unwrap();

        // The database lost its record; the keychain kept the key.
        let fresh = Mutex::new(CourseLibrary::open_in_memory(ImportLimits::default()).unwrap());
        let recovered = json(&describe(&fresh, &credentials).unwrap());
        assert_eq!(recovered["status"], "ready");
        assert_eq!(recovered["signingKey"], created.signing_key.as_str());
        assert!(record(&fresh.lock().unwrap()).unwrap().is_some());

        // The keychain lost the key; the Library still knows which it was.
        store.delete(ACCOUNT).unwrap();
        let missing = json(&describe(&library, &credentials).unwrap());
        assert_eq!(missing["status"], "key_missing");
        assert_eq!(missing["signingKey"], created.signing_key.as_str());
        assert!(missing["problem"].as_str().is_some());
    }

    #[test]
    fn a_backup_restores_the_key_on_another_computer() {
        let (library, credentials, _) = fixture();
        assert_eq!(
            backup(&library, &credentials, PASSPHRASE).unwrap_err().code,
            "learner_key_missing"
        );
        let created = create_key(&library, &credentials).unwrap();
        assert_eq!(
            backup(&library, &credentials, "short").unwrap_err().code,
            "weak_passphrase"
        );
        let file = backup(&library, &credentials, PASSPHRASE).unwrap();

        let (elsewhere, elsewhere_credentials, _) = fixture();
        assert_eq!(
            restore(
                &elsewhere,
                &elsewhere_credentials,
                &file,
                "wrong passphrase!"
            )
            .unwrap_err()
            .code,
            "wrong_passphrase"
        );
        assert_eq!(
            restore(&elsewhere, &elsewhere_credentials, b"{}", PASSPHRASE)
                .unwrap_err()
                .code,
            "backup_invalid"
        );
        restore(&elsewhere, &elsewhere_credentials, &file, PASSPHRASE).unwrap();
        let restored = json(&describe(&elsewhere, &elsewhere_credentials).unwrap());
        assert_eq!(restored["status"], "ready");
        assert_eq!(restored["signingKey"], created.signing_key.as_str());

        // A computer with its own key keeps it.
        let (other, other_credentials, _) = fixture();
        create_key(&other, &other_credentials).unwrap();
        assert_eq!(
            restore(&other, &other_credentials, &file, PASSPHRASE)
                .unwrap_err()
                .code,
            "learner_key_exists"
        );
    }
}
