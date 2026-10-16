//! The publisher's signing identity: key generation, the operating system's
//! credential store, rotation, and passphrase-encrypted backups (ADR 0014).
//!
//! Secret keys live only in a [`CredentialStore`]: the macOS Keychain, the
//! Windows Credential Manager, or the Secret Service on Linux. The public
//! [`Identity`] is plain data for the publisher database.

mod backup;
pub mod passphrase;
mod store;

use course_signing::{PublicKey, RotationStatement, SecretKey, Signed, fingerprint, rotate};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use backup::{
    BACKUP_FORMAT, MAX_BACKUP_BYTES, MIN_PASSPHRASE_CHARS, export_backup, restore_backup,
};
pub use store::{CredentialStore, MemoryStore, OsStore};

/// Credential store service name for every Zeron publisher key.
pub const SERVICE: &str = "app.zeron.publisher";

/// The public parts of a publisher identity. Nothing here is secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    /// The first key, the identity learners pin (`publisherId`).
    pub root: String,
    /// The key that signs now.
    pub current: String,
    /// Unix seconds when the root key was created.
    pub created_at: u64,
    /// Rotations from the root to the current key.
    pub rotations: Vec<Signed<RotationStatement>>,
}

/// Why an identity operation failed. Messages never contain secrets.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// The credential store refused or is unavailable.
    #[error("the system keychain is unavailable: {0}")]
    Store(String),
    /// The credential store has no key for this identity.
    #[error("this computer's keychain has no signing key for this publisher identity")]
    MissingKey,
    /// The stored key does not match the identity.
    #[error("the keychain holds a different signing key than this publisher identity")]
    WrongKey,
    /// The OS random source failed.
    #[error("cannot create a key: {0}")]
    Random(String),
    /// A backup file is malformed, too large, or of another format.
    #[error("this file is not a Zeron publisher key backup")]
    InvalidBackup,
    /// A backup's passphrase is wrong or the file was changed.
    #[error("the passphrase is wrong, or the backup was changed")]
    WrongPassphrase,
    /// A new passphrase is too short.
    #[error("use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters")]
    WeakPassphrase,
    /// The identity's public data is inconsistent.
    #[error(transparent)]
    Signing(#[from] course_signing::SigningError),
}

impl Identity {
    /// The root key, parsed.
    ///
    /// # Errors
    /// Returns [`IdentityError::Signing`] for a malformed record.
    pub fn root_key(&self) -> Result<PublicKey, IdentityError> {
        Ok(self.root.parse()?)
    }

    /// The current key, parsed.
    ///
    /// # Errors
    /// Returns [`IdentityError::Signing`] for a malformed record.
    pub fn current_key(&self) -> Result<PublicKey, IdentityError> {
        Ok(self.current.parse()?)
    }

    /// The root key's fingerprint, for comparing out of band.
    ///
    /// # Errors
    /// Returns [`IdentityError::Signing`] for a malformed record.
    pub fn fingerprint(&self) -> Result<String, IdentityError> {
        Ok(fingerprint(&self.root_key()?))
    }

    /// Check that the rotation chain leads from the root to the current key.
    ///
    /// # Errors
    /// Returns [`IdentityError::Signing`].
    pub fn verify(&self) -> Result<(), IdentityError> {
        let chain = course_signing::verify_chain(&self.root_key()?, &self.rotations)?;
        if chain.last() == Some(&self.current_key()?) {
            Ok(())
        } else {
            Err(course_signing::SigningError::BrokenChain.into())
        }
    }
}

/// Create a new identity with a fresh key held in `store`.
///
/// # Errors
/// Returns a random-source or credential-store failure.
pub fn create(store: &dyn CredentialStore, now: u64) -> Result<Identity, IdentityError> {
    let secret = generate()?;
    let key = secret.public_key();
    store.save(&account(&key), &secret.to_bytes())?;
    Ok(Identity {
        root: key.to_string(),
        current: key.to_string(),
        created_at: now,
        rotations: Vec::new(),
    })
}

/// The current secret key, read from `store` and checked against the
/// identity.
///
/// # Errors
/// Returns [`IdentityError::MissingKey`], [`IdentityError::WrongKey`], or a
/// credential-store failure.
pub fn signing_key(
    store: &dyn CredentialStore,
    identity: &Identity,
) -> Result<SecretKey, IdentityError> {
    let current = identity.current_key()?;
    let bytes = store
        .load(&account(&current))?
        .ok_or(IdentityError::MissingKey)?;
    let secret = secret_from(&bytes)?;
    if secret.public_key() == current {
        Ok(secret)
    } else {
        Err(IdentityError::WrongKey)
    }
}

/// Retire the current key: create a new one, sign the rotation with the old
/// one, and store the new secret under its own entry. The caller persists
/// the returned identity and then calls [`forget_key`] for the old key, so a
/// crash in between leaves both keys available.
///
/// # Errors
/// Returns a credential-store or random-source failure.
pub fn rotate_key(
    store: &dyn CredentialStore,
    identity: &Identity,
    now: u64,
) -> Result<Identity, IdentityError> {
    let previous = signing_key(store, identity)?;
    let next = generate()?;
    let next_key = next.public_key();
    let statement = rotate(&previous, &next_key, now);
    store.save(&account(&next_key), &next.to_bytes())?;
    let mut rotations = identity.rotations.clone();
    rotations.push(statement);
    Ok(Identity {
        root: identity.root.clone(),
        current: next_key.to_string(),
        created_at: identity.created_at,
        rotations,
    })
}

/// Remove one key's secret from the store; a missing entry is not an error.
///
/// # Errors
/// Returns a credential-store failure.
pub fn forget_key(store: &dyn CredentialStore, key: &PublicKey) -> Result<(), IdentityError> {
    store.delete(&account(key))
}

/// The credential store account of one key: `signing-key-<hex>`.
#[must_use]
pub fn account(key: &PublicKey) -> String {
    let text = key.to_string();
    format!("signing-key-{}", text.trim_start_matches("ed25519-"))
}

fn generate() -> Result<SecretKey, IdentityError> {
    let mut bytes = zeroize::Zeroizing::new([0_u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|error| IdentityError::Random(error.to_string()))?;
    Ok(SecretKey::from_bytes(&bytes))
}

fn secret_from(bytes: &[u8]) -> Result<SecretKey, IdentityError> {
    let bytes: zeroize::Zeroizing<[u8; 32]> =
        zeroize::Zeroizing::new(bytes.try_into().map_err(|_| IdentityError::WrongKey)?);
    Ok(SecretKey::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_signs_with_its_stored_key_and_survives_rotation() {
        let store = MemoryStore::default();
        let identity = create(&store, 100).unwrap();
        assert_eq!(identity.root, identity.current);
        identity.verify().unwrap();
        let first = signing_key(&store, &identity).unwrap();
        assert_eq!(first.public_key().to_string(), identity.current);
        assert_eq!(identity.fingerprint().unwrap().len(), 19);

        let rotated = rotate_key(&store, &identity, 200).unwrap();
        assert_eq!(rotated.root, identity.root);
        assert_ne!(rotated.current, identity.current);
        rotated.verify().unwrap();
        assert_eq!(
            signing_key(&store, &rotated)
                .unwrap()
                .public_key()
                .to_string(),
            rotated.current
        );
        // Until the caller forgets it, the old key still signs for the old record.
        assert!(signing_key(&store, &identity).is_ok());
        forget_key(&store, &identity.current_key().unwrap()).unwrap();
        forget_key(&store, &identity.current_key().unwrap()).unwrap();
        assert!(matches!(
            signing_key(&store, &identity),
            Err(IdentityError::MissingKey)
        ));
        assert!(signing_key(&store, &rotated).is_ok());
    }

    #[test]
    fn a_mismatched_stored_key_is_refused() {
        let store = MemoryStore::default();
        let identity = create(&store, 1).unwrap();
        let other = create(&MemoryStore::default(), 1).unwrap();
        let stolen = signing_key(&store, &identity).unwrap();
        store
            .save(&account(&other.current_key().unwrap()), &stolen.to_bytes())
            .unwrap();
        assert!(matches!(
            signing_key(&store, &other),
            Err(IdentityError::WrongKey)
        ));
        store
            .save(&account(&other.current_key().unwrap()), &[1, 2, 3])
            .unwrap();
        assert!(matches!(
            signing_key(&store, &other),
            Err(IdentityError::WrongKey)
        ));
    }
}
