//! Passphrase encryption shared by every Zeron key backup: Argon2id (64 MiB,
//! 3 passes, a random 16-byte salt) derives an XChaCha20-Poly1305 key, with a
//! random 24-byte nonce and caller-chosen associated data (ADR 0014
//! decision 5). The publisher key backup and the learner key backup
//! (ADR 0017) differ only in their format names and associated data.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::IdentityError;

/// Shortest passphrase accepted for a new backup.
pub const MIN_PASSPHRASE_CHARS: usize = 12;
const KDF: &str = "argon2id";
const CIPHER: &str = "xchacha20-poly1305";
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 1;
/// Bounds on the cost a backup may ask for, so a crafted file cannot make
/// restoring take unbounded memory or time.
pub(crate) const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_ITERATIONS: u32 = 10;
const MAX_PARALLELISM: u32 = 16;

/// Key-derivation parameters as written in a backup file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KdfParameters {
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

/// Cipher parameters as written in a backup file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CipherParameters {
    algorithm: String,
    nonce: String,
}

/// A secret sealed under a passphrase: the three backup-file fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    /// `kdf`.
    pub kdf: KdfParameters,
    /// `cipher`.
    pub cipher: CipherParameters,
    /// `ciphertext`, lowercase hex.
    pub ciphertext: String,
}

/// Seal `secret` under `passphrase`, binding `associated_data`.
///
/// # Errors
/// Returns [`IdentityError::WeakPassphrase`] or a random-source failure.
pub fn seal(
    secret: &[u8],
    passphrase: &str,
    associated_data: &[u8],
) -> Result<Sealed, IdentityError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(IdentityError::WeakPassphrase);
    }
    let mut salt = [0_u8; 16];
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut salt).map_err(|error| IdentityError::Random(error.to_string()))?;
    getrandom::fill(&mut nonce).map_err(|error| IdentityError::Random(error.to_string()))?;
    let key = derive(passphrase, &salt, MEMORY_KIB, ITERATIONS, PARALLELISM)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| IdentityError::InvalidBackup)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: secret,
                aad: associated_data,
            },
        )
        .map_err(|_| IdentityError::InvalidBackup)?;
    Ok(Sealed {
        kdf: KdfParameters {
            algorithm: KDF.to_owned(),
            memory_kib: MEMORY_KIB,
            iterations: ITERATIONS,
            parallelism: PARALLELISM,
            salt: hex(&salt),
        },
        cipher: CipherParameters {
            algorithm: CIPHER.to_owned(),
            nonce: hex(&nonce),
        },
        ciphertext: hex(&ciphertext),
    })
}

/// Open a sealed secret with `passphrase` and the same `associated_data`.
///
/// # Errors
/// Returns [`IdentityError::InvalidBackup`] for unknown algorithms,
/// excessive costs, or malformed fields, and
/// [`IdentityError::WrongPassphrase`] when authentication fails.
pub fn open(
    sealed: &Sealed,
    passphrase: &str,
    associated_data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, IdentityError> {
    let kdf = &sealed.kdf;
    let well_formed = kdf.algorithm == KDF
        && sealed.cipher.algorithm == CIPHER
        && kdf.memory_kib <= MAX_MEMORY_KIB
        && kdf.iterations <= MAX_ITERATIONS
        && kdf.parallelism <= MAX_PARALLELISM;
    if !well_formed {
        return Err(IdentityError::InvalidBackup);
    }
    let salt = unhex(&kdf.salt).filter(|salt| salt.len() == 16);
    let nonce: Option<[u8; 24]> =
        unhex(&sealed.cipher.nonce).and_then(|nonce| nonce.try_into().ok());
    let ciphertext = unhex(&sealed.ciphertext);
    let (Some(salt), Some(nonce), Some(ciphertext)) = (salt, nonce, ciphertext) else {
        return Err(IdentityError::InvalidBackup);
    };
    let key = derive(
        passphrase,
        &salt,
        kdf.memory_kib,
        kdf.iterations,
        kdf.parallelism,
    )?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| IdentityError::InvalidBackup)?;
    cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &ciphertext,
                aad: associated_data,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| IdentityError::WrongPassphrase)
}

fn derive(
    passphrase: &str,
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<Zeroizing<[u8; 32]>, IdentityError> {
    let params = Params::new(memory_kib, iterations, parallelism, Some(32))
        .map_err(|_| IdentityError::InvalidBackup)?;
    let mut key = Zeroizing::new([0_u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| IdentityError::InvalidBackup)?;
    Ok(key)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |value: u8| match value {
                b'0'..=b'9' => Some(value - b'0'),
                b'a'..=b'f' => Some(value - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? << 4 | digit(pair[1])?)
        })
        .collect()
}
