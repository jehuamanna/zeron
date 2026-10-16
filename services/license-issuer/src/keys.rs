//! The issuer's keys: one random 32-byte seed in `issuer-key.json`, readable
//! only by its owner, from which HKDF-SHA256 derives the Ed25519 key that
//! signs licenses and the X25519 key that release content keys are sealed
//! to. The seed never enters the database, so a database copy alone opens no
//! content key.

use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::Path,
};

use course_signing::{PublicKey, SecretKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::IssuerError;

/// The key file's name in the data directory.
pub const KEY_FILE: &str = "issuer-key.json";
const FORMAT: &str = "zeron.issuer-key";
const SALT: &[u8] = b"zeron.issuer-key.v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyFile {
    format: String,
    version: u32,
    seed: String,
}

/// The issuer's secret and public keys. `Debug` shows only public keys.
pub struct IssuerKeys {
    signing: SecretKey,
    encryption_secret: Zeroizing<[u8; 32]>,
    encryption_public: [u8; 32],
}

impl std::fmt::Debug for IssuerKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuerKeys")
            .field("signing", &self.signing.public_key().to_string())
            .finish_non_exhaustive()
    }
}

impl IssuerKeys {
    /// Keys derived from a seed.
    #[must_use]
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let derive = |info: &[u8]| {
            let mut key = Zeroizing::new([0_u8; 32]);
            Hkdf::<Sha256>::new(Some(SALT), seed)
                .expand(info, key.as_mut())
                .unwrap_or_else(|_| unreachable!("32 bytes is a valid HKDF-SHA256 length"));
            key
        };
        let encryption_secret = derive(b"encryption");
        let encryption_public =
            *x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*encryption_secret))
                .as_bytes();
        Self {
            signing: SecretKey::from_bytes(&derive(b"signing")),
            encryption_secret,
            encryption_public,
        }
    }

    /// Create a new key file in `directory`, refusing to replace one.
    ///
    /// # Errors
    /// Returns [`IssuerError::KeyFile`] when the file exists or cannot be
    /// written, or a random-source failure.
    pub fn create(directory: &Path) -> Result<Self, IssuerError> {
        let mut seed = Zeroizing::new([0_u8; 32]);
        getrandom::fill(seed.as_mut())
            .map_err(|_| IssuerError::KeyFile("the random source failed".to_owned()))?;
        let file = KeyFile {
            format: FORMAT.to_owned(),
            version: 1,
            seed: hex(seed.as_ref()),
        };
        let bytes = Zeroizing::new(
            serde_json::to_vec_pretty(&file)
                .map_err(|_| IssuerError::KeyFile("cannot encode the key file".to_owned()))?,
        );
        fs::create_dir_all(directory).map_err(|error| {
            IssuerError::KeyFile(format!("cannot create the data directory: {error}"))
        })?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut handle = options
            .open(directory.join(KEY_FILE))
            .map_err(|error| IssuerError::KeyFile(format!("cannot create {KEY_FILE}: {error}")))?;
        handle
            .write_all(&bytes)
            .and_then(|()| handle.sync_all())
            .map_err(|error| IssuerError::KeyFile(format!("cannot write {KEY_FILE}: {error}")))?;
        Ok(Self::from_seed(&seed))
    }

    /// Load the key file from `directory`.
    ///
    /// # Errors
    /// Returns [`IssuerError::KeyFile`] when it is missing or malformed.
    pub fn load(directory: &Path) -> Result<Self, IssuerError> {
        let bytes = Zeroizing::new(fs::read(directory.join(KEY_FILE)).map_err(|error| {
            IssuerError::KeyFile(format!("cannot read {KEY_FILE}: {error}; run `init` first"))
        })?);
        let file: KeyFile = serde_json::from_slice(&bytes)
            .map_err(|_| IssuerError::KeyFile(format!("{KEY_FILE} is malformed")))?;
        let seed = Zeroizing::new(
            unhex32(&file.seed)
                .filter(|_| file.format == FORMAT && file.version == 1)
                .ok_or_else(|| IssuerError::KeyFile(format!("{KEY_FILE} is malformed")))?,
        );
        Ok(Self::from_seed(&seed))
    }

    /// The key that signs licenses; publishers delegate to it.
    #[must_use]
    pub fn signing_public(&self) -> PublicKey {
        self.signing.public_key()
    }

    /// The key that signs licenses.
    #[must_use]
    pub const fn signing(&self) -> &SecretKey {
        &self.signing
    }

    /// The X25519 key publishers seal release content keys to.
    #[must_use]
    pub const fn encryption_public(&self) -> &[u8; 32] {
        &self.encryption_public
    }

    /// The X25519 secret that opens sealed release content keys.
    #[must_use]
    pub fn encryption_secret(&self) -> &[u8; 32] {
        &self.encryption_secret
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

pub(crate) fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks(2).enumerate() {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        };
        out[index] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_is_created_once_private_and_reloads_the_same_keys() {
        let directory =
            std::env::temp_dir().join(format!("zeron-issuer-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let created = IssuerKeys::create(&directory).unwrap();
        assert!(
            IssuerKeys::create(&directory).is_err(),
            "never replaces a key"
        );
        let loaded = IssuerKeys::load(&directory).unwrap();
        assert_eq!(loaded.signing_public(), created.signing_public());
        assert_eq!(loaded.encryption_public(), created.encryption_public());
        assert!(!format!("{loaded:?}").contains(&hex(loaded.encryption_secret())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(directory.join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "only the owner may read the key file");
        }
        fs::write(directory.join(KEY_FILE), b"{}").unwrap();
        assert!(IssuerKeys::load(&directory).is_err());
        let _ = fs::remove_dir_all(&directory);
    }
}
