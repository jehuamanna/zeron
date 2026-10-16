//! Passphrase-encrypted backups of the signing key (ADR 0014 decision 5).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{CredentialStore, Identity, IdentityError, account, secret_from, signing_key};

/// The `format` value every backup carries.
pub const BACKUP_FORMAT: &str = "zeron.publisher-key-backup";
/// Largest backup file read.
pub const MAX_BACKUP_BYTES: usize = 128 * 1024;
/// Shortest passphrase accepted for a new backup.
pub const MIN_PASSPHRASE_CHARS: usize = 12;
const VERSION: u32 = 1;
const KDF: &str = "argon2id";
const CIPHER: &str = "xchacha20-poly1305";
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 1;
/// Bounds on the cost a backup may ask for, so a crafted file cannot make
/// restoring take unbounded memory or time.
const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_ITERATIONS: u32 = 10;
const AAD_DOMAIN: &[u8] = b"zeron.publisher-key-backup.v1\n";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackupFile {
    format: String,
    version: u32,
    identity: Identity,
    kdf: KdfParameters,
    cipher: CipherParameters,
    ciphertext: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KdfParameters {
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CipherParameters {
    algorithm: String,
    nonce: String,
}

/// Encrypt the identity's current key under `passphrase`. The public
/// identity travels in the clear and is bound to the ciphertext as
/// associated data, so neither can be swapped.
///
/// # Errors
/// Returns [`IdentityError::WeakPassphrase`], a credential-store failure,
/// or a random-source failure.
pub fn export_backup(
    store: &dyn CredentialStore,
    identity: &Identity,
    passphrase: &str,
) -> Result<Vec<u8>, IdentityError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(IdentityError::WeakPassphrase);
    }
    let secret = signing_key(store, identity)?;
    let mut salt = [0_u8; 16];
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut salt).map_err(|error| IdentityError::Random(error.to_string()))?;
    getrandom::fill(&mut nonce).map_err(|error| IdentityError::Random(error.to_string()))?;
    let key = derive(passphrase, &salt, MEMORY_KIB, ITERATIONS, PARALLELISM)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| IdentityError::InvalidBackup)?;
    let secret_bytes = Zeroizing::new(secret.to_bytes());
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: secret_bytes.as_ref(),
                aad: &associated_data(identity),
            },
        )
        .map_err(|_| IdentityError::InvalidBackup)?;
    let file = BackupFile {
        format: BACKUP_FORMAT.to_owned(),
        version: VERSION,
        identity: identity.clone(),
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
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|_| IdentityError::InvalidBackup)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Decrypt a backup, check that the key matches its identity, and store the
/// key in `store`. Returns the identity to record.
///
/// # Errors
/// Returns [`IdentityError::InvalidBackup`],
/// [`IdentityError::WrongPassphrase`], or a credential-store failure.
pub fn restore_backup(
    store: &dyn CredentialStore,
    bytes: &[u8],
    passphrase: &str,
) -> Result<Identity, IdentityError> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(IdentityError::InvalidBackup);
    }
    let file: BackupFile =
        serde_json::from_slice(bytes).map_err(|_| IdentityError::InvalidBackup)?;
    let kdf = &file.kdf;
    let well_formed = file.format == BACKUP_FORMAT
        && file.version == VERSION
        && kdf.algorithm == KDF
        && file.cipher.algorithm == CIPHER
        && kdf.memory_kib <= MAX_MEMORY_KIB
        && kdf.iterations <= MAX_ITERATIONS
        && kdf.parallelism <= 16;
    if !well_formed {
        return Err(IdentityError::InvalidBackup);
    }
    file.identity
        .verify()
        .map_err(|_| IdentityError::InvalidBackup)?;
    let salt = unhex(&kdf.salt).filter(|salt| salt.len() == 16);
    let nonce: Option<[u8; 24]> = unhex(&file.cipher.nonce).and_then(|nonce| nonce.try_into().ok());
    let ciphertext = unhex(&file.ciphertext);
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
    let secret = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &associated_data(&file.identity),
                },
            )
            .map_err(|_| IdentityError::WrongPassphrase)?,
    );
    let secret = secret_from(&secret).map_err(|_| IdentityError::InvalidBackup)?;
    let current = file.identity.current_key()?;
    if secret.public_key() != current {
        return Err(IdentityError::InvalidBackup);
    }
    store.save(&account(&current), &secret.to_bytes())?;
    Ok(file.identity)
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

fn associated_data(identity: &Identity) -> Vec<u8> {
    let mut data = AAD_DOMAIN.to_vec();
    data.extend(serde_json::to_vec(identity).unwrap_or_default());
    data
}

fn hex(bytes: &[u8]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryStore, create, rotate_key};

    const PASSPHRASE: &str = "correct horse battery";

    #[test]
    fn a_backup_restores_the_identity_on_another_computer() {
        let here = MemoryStore::default();
        let identity = rotate_key(&here, &create(&here, 1).unwrap(), 2).unwrap();
        let backup = export_backup(&here, &identity, PASSPHRASE).unwrap();
        let text = String::from_utf8(backup.clone()).unwrap();
        let secret = signing_key(&here, &identity).unwrap();
        assert!(!text.contains(&hex(&secret.to_bytes())));
        assert!(text.contains(&identity.root));

        let there = MemoryStore::default();
        let restored = restore_backup(&there, &backup, PASSPHRASE).unwrap();
        assert_eq!(restored, identity);
        assert_eq!(
            signing_key(&there, &restored).unwrap().public_key(),
            secret.public_key()
        );
    }

    #[test]
    fn wrong_passphrases_weak_passphrases_and_tampering_are_refused() {
        let store = MemoryStore::default();
        let identity = create(&store, 1).unwrap();
        assert!(matches!(
            export_backup(&store, &identity, "short"),
            Err(IdentityError::WeakPassphrase)
        ));
        let backup = export_backup(&store, &identity, PASSPHRASE).unwrap();
        let target = MemoryStore::default();
        assert!(matches!(
            restore_backup(&target, &backup, "correct horse battery!"),
            Err(IdentityError::WrongPassphrase)
        ));

        let mut file: serde_json::Value = serde_json::from_slice(&backup).unwrap();
        let other = create(&MemoryStore::default(), 1).unwrap();
        file["identity"] = serde_json::to_value(&other).unwrap();
        assert!(matches!(
            restore_backup(&target, &serde_json::to_vec(&file).unwrap(), PASSPHRASE),
            Err(IdentityError::WrongPassphrase)
        ));
        let mut costly: serde_json::Value = serde_json::from_slice(&backup).unwrap();
        costly["kdf"]["memoryKib"] = (MAX_MEMORY_KIB + 1).into();
        assert!(matches!(
            restore_backup(&target, &serde_json::to_vec(&costly).unwrap(), PASSPHRASE),
            Err(IdentityError::InvalidBackup)
        ));
        assert!(matches!(
            restore_backup(&target, b"{}", PASSPHRASE),
            Err(IdentityError::InvalidBackup)
        ));
        assert!(matches!(
            restore_backup(&target, &vec![b' '; MAX_BACKUP_BYTES + 1], PASSPHRASE),
            Err(IdentityError::InvalidBackup)
        ));
        assert!(matches!(
            signing_key(&target, &identity),
            Err(IdentityError::MissingKey)
        ));
    }
}
