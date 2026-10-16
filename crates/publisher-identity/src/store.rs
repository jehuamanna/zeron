//! Credential stores holding secret keys.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use zeroize::Zeroizing;

use crate::{IdentityError, SERVICE};

/// Where secret keys are kept. Implementations must never log secrets.
pub trait CredentialStore: Send + Sync {
    /// The secret stored for `account`, or `None`.
    ///
    /// # Errors
    /// Returns [`IdentityError::Store`].
    fn load(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, IdentityError>;

    /// Store or replace the secret for `account`.
    ///
    /// # Errors
    /// Returns [`IdentityError::Store`].
    fn save(&self, account: &str, secret: &[u8]) -> Result<(), IdentityError>;

    /// Remove the secret for `account`; a missing entry is not an error.
    ///
    /// # Errors
    /// Returns [`IdentityError::Store`].
    fn delete(&self, account: &str) -> Result<(), IdentityError>;
}

/// The operating system's credential store: the macOS Keychain, the Windows
/// Credential Manager, or the Secret Service on Linux and the BSDs.
pub struct OsStore {
    store: Arc<keyring_core::CredentialStore>,
}

impl OsStore {
    /// Connect to this platform's credential store.
    ///
    /// # Errors
    /// Returns [`IdentityError::Store`] when the platform has none, or on
    /// Linux when no Secret Service (GNOME Keyring or `KWallet`) is running.
    pub fn open() -> Result<Self, IdentityError> {
        Ok(Self {
            store: platform_store().map_err(store_error)?,
        })
    }

    fn entry(&self, account: &str) -> Result<keyring_core::Entry, IdentityError> {
        self.store
            .build(SERVICE, account, None)
            .map_err(store_error)
    }
}

impl CredentialStore for OsStore {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, IdentityError> {
        match self.entry(account)?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn save(&self, account: &str, secret: &[u8]) -> Result<(), IdentityError> {
        self.entry(account)?.set_secret(secret).map_err(store_error)
    }

    fn delete(&self, account: &str) -> Result<(), IdentityError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(target_os = "windows")]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Ok(dbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd"
)))]
fn platform_store() -> keyring_core::Result<Arc<keyring_core::CredentialStore>> {
    Err(keyring_core::Error::NotSupportedByStore(
        "no credential store on this platform".to_owned(),
    ))
}

/// The store's own message, which names neither the secret nor its value.
fn store_error(error: keyring_core::Error) -> IdentityError {
    IdentityError::Store(match error {
        keyring_core::Error::NoStorageAccess(_) => {
            "access was denied or the keychain is locked".to_owned()
        }
        keyring_core::Error::PlatformFailure(_) => "the keychain service failed".to_owned(),
        other => other.to_string(),
    })
}

/// An in-memory store for tests and for computers without a keychain.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
}

impl CredentialStore for MemoryStore {
    fn load(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, IdentityError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(account)
            .cloned())
    }

    fn save(&self, account: &str, secret: &[u8]) -> Result<(), IdentityError> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(account.to_owned(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), IdentityError> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(account);
        Ok(())
    }
}
