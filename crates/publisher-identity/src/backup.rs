//! Passphrase-encrypted backups of the signing key (ADR 0014 decision 5).

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    CredentialStore, Identity, IdentityError, account,
    passphrase::{self, CipherParameters, KdfParameters, Sealed},
    secret_from, signing_key,
};

pub use crate::passphrase::MIN_PASSPHRASE_CHARS;

/// The `format` value every backup carries.
pub const BACKUP_FORMAT: &str = "zeron.publisher-key-backup";
/// Largest backup file read.
pub const MAX_BACKUP_BYTES: usize = 128 * 1024;
const VERSION: u32 = 1;
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
    let secret_bytes = Zeroizing::new(secret.to_bytes());
    let sealed = passphrase::seal(
        secret_bytes.as_ref(),
        passphrase,
        &associated_data(identity),
    )?;
    let file = BackupFile {
        format: BACKUP_FORMAT.to_owned(),
        version: VERSION,
        identity: identity.clone(),
        kdf: sealed.kdf,
        cipher: sealed.cipher,
        ciphertext: sealed.ciphertext,
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
    if file.format != BACKUP_FORMAT || file.version != VERSION {
        return Err(IdentityError::InvalidBackup);
    }
    file.identity
        .verify()
        .map_err(|_| IdentityError::InvalidBackup)?;
    let secret = passphrase::open(
        &Sealed {
            kdf: file.kdf,
            cipher: file.cipher,
            ciphertext: file.ciphertext,
        },
        passphrase,
        &associated_data(&file.identity),
    )?;
    let secret = secret_from(&secret).map_err(|_| IdentityError::InvalidBackup)?;
    let current = file.identity.current_key()?;
    if secret.public_key() != current {
        return Err(IdentityError::InvalidBackup);
    }
    store.save(&account(&current), &secret.to_bytes())?;
    Ok(file.identity)
}

fn associated_data(identity: &Identity) -> Vec<u8> {
    let mut data = AAD_DOMAIN.to_vec();
    data.extend(serde_json::to_vec(identity).unwrap_or_default());
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MemoryStore, create,
        passphrase::{MAX_MEMORY_KIB, hex},
        rotate_key,
    };

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
