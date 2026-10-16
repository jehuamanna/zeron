//! The learner's key on this computer (ADR 0017 decision 3): one random
//! 32-byte seed in the operating system's credential store, service
//! [`SERVICE`], from which two keys are derived with HKDF-SHA256:
//!
//! - an Ed25519 key that signs requests to a license issuer;
//! - an X25519 key that licenses seal content keys to.
//!
//! Only the public [`LearnerIdentity`] is plain data for the Library's
//! database. The seed never leaves the credential store except inside a
//! passphrase-encrypted backup, in the same envelope as publisher key
//! backups (ADR 0014 decision 5). No accounts: the key is the identity.

use course_signing::{PublicKey, SecretKey, fingerprint};
use hkdf::Hkdf;
use publisher_identity::{
    IdentityError,
    passphrase::{self, CipherParameters, KdfParameters, Sealed},
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroizing;

pub use publisher_identity::{
    CredentialStore, MemoryStore, OsStore, passphrase::MIN_PASSPHRASE_CHARS,
};

/// Credential store service of the learner key.
pub const SERVICE: &str = "app.zeron.learner";
/// Credential store account holding the learner key's seed.
pub const ACCOUNT: &str = "learner-key";
/// The `format` value of every learner key backup.
pub const BACKUP_FORMAT: &str = "zeron.learner-key-backup";
/// Largest backup file read.
pub const MAX_BACKUP_BYTES: usize = 16 * 1024;
const BACKUP_VERSION: u32 = 1;
const AAD_DOMAIN: &[u8] = b"zeron.learner-key-backup.v1\n";
const DERIVATION_SALT: &[u8] = b"zeron.learner-key.v1";
const SIGNING_INFO: &[u8] = b"signing";
const ENCRYPTION_INFO: &[u8] = b"encryption";
const ENCRYPTION_PREFIX: &str = "x25519-";
/// Prefix of a learner ID, the public keys a learner shares to be licensed.
pub const LEARNER_ID_PREFIX: &str = "zeron-learner:v1:";

/// Why a learner key operation failed. Messages never contain secrets.
#[derive(Debug, Error)]
pub enum LearnerKeyError {
    /// The credential store refused or is unavailable.
    #[error("the system keychain is unavailable: {0}")]
    Store(String),
    /// The credential store has no learner key.
    #[error("this computer's keychain has no learner key")]
    Missing,
    /// The credential store holds a different learner key than expected.
    #[error("the keychain holds a different learner key")]
    WrongKey,
    /// The credential store already holds a learner key.
    #[error("this computer already has a learner key")]
    Exists,
    /// The OS random source failed.
    #[error("cannot create a key: {0}")]
    Random(String),
    /// A backup file is malformed, too large, or of another format.
    #[error("this file is not a Zeron learner key backup")]
    InvalidBackup,
    /// A backup's passphrase is wrong or the file was changed.
    #[error("the passphrase is wrong, or the backup was changed")]
    WrongPassphrase,
    /// A new passphrase is too short.
    #[error("use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters")]
    WeakPassphrase,
}

impl From<IdentityError> for LearnerKeyError {
    fn from(error: IdentityError) -> Self {
        match error {
            IdentityError::Store(message) => Self::Store(message),
            IdentityError::MissingKey => Self::Missing,
            IdentityError::WrongKey | IdentityError::Signing(_) => Self::WrongKey,
            IdentityError::Random(message) => Self::Random(message),
            IdentityError::InvalidBackup => Self::InvalidBackup,
            IdentityError::WrongPassphrase => Self::WrongPassphrase,
            IdentityError::WeakPassphrase => Self::WeakPassphrase,
        }
    }
}

/// The public parts of a learner key. Nothing here is secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearnerIdentity {
    /// `ed25519-<hex>`: verifies requests the learner signs.
    pub signing_key: String,
    /// `x25519-<hex>`: licenses seal content keys to it.
    pub encryption_key: String,
    /// Unix seconds when the key was created on, or restored to, this
    /// computer's records.
    pub created_at: u64,
}

impl LearnerIdentity {
    /// The signing key's fingerprint, for comparing out of band.
    ///
    /// # Errors
    /// Returns [`LearnerKeyError::WrongKey`] for a malformed record.
    pub fn fingerprint(&self) -> Result<String, LearnerKeyError> {
        let key: PublicKey = self
            .signing_key
            .parse()
            .map_err(|_| LearnerKeyError::WrongKey)?;
        Ok(fingerprint(&key))
    }

    /// The X25519 public key's 32 bytes.
    ///
    /// # Errors
    /// Returns [`LearnerKeyError::WrongKey`] for a malformed record.
    pub fn encryption_public_key(&self) -> Result<[u8; 32], LearnerKeyError> {
        self.encryption_key
            .strip_prefix(ENCRYPTION_PREFIX)
            .and_then(unhex32)
            .ok_or(LearnerKeyError::WrongKey)
    }

    /// The learner ID to share with a publisher or issuer:
    /// `zeron-learner:v1:<signing key hex>:<encryption key hex>`. Public.
    #[must_use]
    pub fn learner_id(&self) -> String {
        format!(
            "{LEARNER_ID_PREFIX}{}:{}",
            self.signing_key.trim_start_matches("ed25519-"),
            self.encryption_key.trim_start_matches(ENCRYPTION_PREFIX)
        )
    }

    fn matches(&self, keys: &LearnerKeys) -> bool {
        let derived = keys.identity(self.created_at);
        derived.signing_key == self.signing_key && derived.encryption_key == self.encryption_key
    }
}

/// The learner's secret keys, derived from the stored seed. Its `Debug`
/// shows only public keys.
pub struct LearnerKeys {
    signing: SecretKey,
    encryption: x25519_dalek::StaticSecret,
}

impl std::fmt::Debug for LearnerKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LearnerKeys")
            .field("signing", &self.signing.public_key().to_string())
            .finish_non_exhaustive()
    }
}

impl LearnerKeys {
    fn from_seed(seed: &[u8; 32]) -> Self {
        let derive = |info: &[u8]| {
            let mut key = Zeroizing::new([0_u8; 32]);
            Hkdf::<Sha256>::new(Some(DERIVATION_SALT), seed)
                .expand(info, key.as_mut())
                .unwrap_or_else(|_| unreachable!("32 bytes is a valid HKDF-SHA256 length"));
            key
        };
        Self {
            signing: SecretKey::from_bytes(&derive(SIGNING_INFO)),
            encryption: x25519_dalek::StaticSecret::from(*derive(ENCRYPTION_INFO)),
        }
    }

    /// The Ed25519 key that signs issuer requests.
    #[must_use]
    pub const fn signing(&self) -> &SecretKey {
        &self.signing
    }

    /// The X25519 key that opens content keys sealed to this learner.
    #[must_use]
    pub const fn encryption(&self) -> &x25519_dalek::StaticSecret {
        &self.encryption
    }

    /// The X25519 secret's bytes, for opening wrapped content keys.
    #[must_use]
    pub fn encryption_secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.encryption.to_bytes())
    }

    fn identity(&self, created_at: u64) -> LearnerIdentity {
        LearnerIdentity {
            signing_key: self.signing.public_key().to_string(),
            encryption_key: format!(
                "{ENCRYPTION_PREFIX}{}",
                passphrase_hex(x25519_dalek::PublicKey::from(&self.encryption).as_bytes())
            ),
            created_at,
        }
    }
}

fn load_seed(store: &dyn CredentialStore) -> Result<Option<Zeroizing<[u8; 32]>>, LearnerKeyError> {
    let Some(bytes) = store.load(ACCOUNT)? else {
        return Ok(None);
    };
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LearnerKeyError::WrongKey)?;
    Ok(Some(Zeroizing::new(seed)))
}

/// Create this computer's learner key and save its seed to `store`.
///
/// # Errors
/// Returns [`LearnerKeyError::Exists`] when the store already holds one,
/// or a random-source or credential-store failure.
pub fn create(store: &dyn CredentialStore, now: u64) -> Result<LearnerIdentity, LearnerKeyError> {
    if store.load(ACCOUNT)?.is_some() {
        return Err(LearnerKeyError::Exists);
    }
    let mut seed = Zeroizing::new([0_u8; 32]);
    getrandom::fill(seed.as_mut()).map_err(|error| LearnerKeyError::Random(error.to_string()))?;
    store.save(ACCOUNT, seed.as_ref())?;
    Ok(LearnerKeys::from_seed(&seed).identity(now))
}

/// The identity of the learner key the store holds, if any. Lets the Library
/// recover its record when its database was reset but the keychain kept the
/// key.
///
/// # Errors
/// Returns a credential-store failure or [`LearnerKeyError::WrongKey`] for a
/// malformed entry.
pub fn stored_identity(
    store: &dyn CredentialStore,
    created_at: u64,
) -> Result<Option<LearnerIdentity>, LearnerKeyError> {
    Ok(load_seed(store)?.map(|seed| LearnerKeys::from_seed(&seed).identity(created_at)))
}

/// The keys of whatever learner key `store` holds, if any. For opening
/// licenses, which name the learner key they are for.
///
/// # Errors
/// Returns a credential-store failure or [`LearnerKeyError::WrongKey`] for a
/// malformed entry.
pub fn stored_keys(store: &dyn CredentialStore) -> Result<Option<LearnerKeys>, LearnerKeyError> {
    Ok(load_seed(store)?.map(|seed| LearnerKeys::from_seed(&seed)))
}

/// A learner's public keys, read from a shared learner ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnerId {
    /// The signing key licenses are bound to.
    pub signing_key: PublicKey,
    /// The X25519 key content keys are sealed to.
    pub encryption_key: [u8; 32],
}

/// Parse a learner ID as [`LearnerIdentity::learner_id`] writes it.
/// Surrounding whitespace is ignored.
///
/// # Errors
/// Returns [`LearnerKeyError::WrongKey`] for anything else.
pub fn parse_learner_id(text: &str) -> Result<LearnerId, LearnerKeyError> {
    let rest = text
        .trim()
        .strip_prefix(LEARNER_ID_PREFIX)
        .ok_or(LearnerKeyError::WrongKey)?;
    let (signing, encryption) = rest.split_once(':').ok_or(LearnerKeyError::WrongKey)?;
    let signing_key: PublicKey = format!("ed25519-{signing}")
        .parse()
        .map_err(|_| LearnerKeyError::WrongKey)?;
    let encryption_key = unhex32(encryption).ok_or(LearnerKeyError::WrongKey)?;
    Ok(LearnerId {
        signing_key,
        encryption_key,
    })
}

/// The secret keys of `identity`, read from `store` and checked against it.
///
/// # Errors
/// Returns [`LearnerKeyError::Missing`], [`LearnerKeyError::WrongKey`], or
/// a credential-store failure.
pub fn learner_keys(
    store: &dyn CredentialStore,
    identity: &LearnerIdentity,
) -> Result<LearnerKeys, LearnerKeyError> {
    let seed = load_seed(store)?.ok_or(LearnerKeyError::Missing)?;
    let keys = LearnerKeys::from_seed(&seed);
    if identity.matches(&keys) {
        Ok(keys)
    } else {
        Err(LearnerKeyError::WrongKey)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackupFile {
    format: String,
    version: u32,
    identity: LearnerIdentity,
    kdf: KdfParameters,
    cipher: CipherParameters,
    ciphertext: String,
}

/// Encrypt the learner key's seed under `passphrase`. The public identity
/// travels in the clear, bound to the ciphertext as associated data.
///
/// # Errors
/// Returns [`LearnerKeyError::WeakPassphrase`], a missing or different key,
/// or a credential-store or random-source failure.
pub fn export_backup(
    store: &dyn CredentialStore,
    identity: &LearnerIdentity,
    passphrase: &str,
) -> Result<Vec<u8>, LearnerKeyError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(LearnerKeyError::WeakPassphrase);
    }
    learner_keys(store, identity)?;
    let seed = load_seed(store)?.ok_or(LearnerKeyError::Missing)?;
    let sealed = passphrase::seal(seed.as_ref(), passphrase, &associated_data(identity))?;
    let file = BackupFile {
        format: BACKUP_FORMAT.to_owned(),
        version: BACKUP_VERSION,
        identity: identity.clone(),
        kdf: sealed.kdf,
        cipher: sealed.cipher,
        ciphertext: sealed.ciphertext,
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|_| LearnerKeyError::InvalidBackup)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Decrypt a backup, check the seed against its identity, and save it to
/// `store`. Returns the identity to record.
///
/// # Errors
/// Returns [`LearnerKeyError::InvalidBackup`],
/// [`LearnerKeyError::WrongPassphrase`], [`LearnerKeyError::Exists`] when the
/// store already holds a different learner key, or a credential-store
/// failure.
pub fn restore_backup(
    store: &dyn CredentialStore,
    bytes: &[u8],
    passphrase: &str,
) -> Result<LearnerIdentity, LearnerKeyError> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(LearnerKeyError::InvalidBackup);
    }
    let file: BackupFile =
        serde_json::from_slice(bytes).map_err(|_| LearnerKeyError::InvalidBackup)?;
    if file.format != BACKUP_FORMAT || file.version != BACKUP_VERSION {
        return Err(LearnerKeyError::InvalidBackup);
    }
    let seed = passphrase::open(
        &Sealed {
            kdf: file.kdf,
            cipher: file.cipher,
            ciphertext: file.ciphertext,
        },
        passphrase,
        &associated_data(&file.identity),
    )?;
    let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
        seed.as_slice()
            .try_into()
            .map_err(|_| LearnerKeyError::InvalidBackup)?,
    );
    if !file.identity.matches(&LearnerKeys::from_seed(&seed)) {
        return Err(LearnerKeyError::InvalidBackup);
    }
    match load_seed(store)? {
        Some(existing) if *existing != *seed => return Err(LearnerKeyError::Exists),
        Some(_) => {}
        None => store.save(ACCOUNT, seed.as_ref())?,
    }
    Ok(file.identity)
}

fn associated_data(identity: &LearnerIdentity) -> Vec<u8> {
    let mut data = AAD_DOMAIN.to_vec();
    data.extend(serde_json::to_vec(identity).unwrap_or_default());
    data
}

fn passphrase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn unhex32(text: &str) -> Option<[u8; 32]> {
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

    const PASSPHRASE: &str = "correct horse battery";

    #[test]
    fn a_learner_key_is_created_once_and_derives_both_keys() {
        let store = MemoryStore::default();
        assert!(stored_identity(&store, 1).unwrap().is_none());
        let identity = create(&store, 5).unwrap();
        assert!(identity.signing_key.starts_with("ed25519-"));
        assert!(identity.encryption_key.starts_with("x25519-"));
        assert_eq!(identity.fingerprint().unwrap().len(), 19);
        assert_eq!(stored_identity(&store, 5).unwrap(), Some(identity.clone()));
        assert!(matches!(create(&store, 6), Err(LearnerKeyError::Exists)));

        let keys = learner_keys(&store, &identity).unwrap();
        assert_eq!(
            keys.signing().public_key().to_string(),
            identity.signing_key
        );
        assert_eq!(
            *x25519_dalek::PublicKey::from(keys.encryption()).as_bytes(),
            identity.encryption_public_key().unwrap()
        );
        assert!(!format!("{keys:?}").contains("encryption"));
    }

    #[test]
    fn derivation_is_stable_and_separates_the_two_keys() {
        // Licenses and issuers will rely on this mapping across versions.
        // Both values were computed independently with pure-Python HKDF,
        // Ed25519 (checked against RFC 8032 test 1), and X25519 (checked
        // against RFC 7748 section 6.1).
        let keys = LearnerKeys::from_seed(&[7; 32]);
        let identity = keys.identity(0);
        assert_eq!(
            identity.signing_key,
            "ed25519-bf59c88e33d1f899c2b5d1a7d62ba3708b038799b5f14d0762313950ba36a905"
        );
        assert_eq!(
            identity.encryption_key,
            "x25519-304ebcb034f6c42814d9d4c3a1c53fadd50c00fcfb97998656697ae08c26d773"
        );
    }

    #[test]
    fn learner_ids_round_trip_and_reject_anything_else() {
        let store = MemoryStore::default();
        let identity = create(&store, 1).unwrap();
        let id = identity.learner_id();
        assert!(id.starts_with(LEARNER_ID_PREFIX));
        let parsed = parse_learner_id(&format!("  {id}\n")).unwrap();
        assert_eq!(parsed.signing_key.to_string(), identity.signing_key);
        assert_eq!(
            parsed.encryption_key,
            identity.encryption_public_key().unwrap()
        );
        for bad in [
            "",
            "zeron-learner:v2:aa:bb",
            &id.replace(':', "-"),
            &id[..id.len() - 2],
            &id.to_uppercase(),
        ] {
            assert!(parse_learner_id(bad).is_err(), "accepted {bad:?}");
        }
        let keys = stored_keys(&store).unwrap().unwrap();
        assert_eq!(
            keys.signing().public_key().to_string(),
            identity.signing_key
        );
        assert_eq!(
            *x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(
                *keys.encryption_secret_bytes()
            ))
            .as_bytes(),
            identity.encryption_public_key().unwrap()
        );
        assert!(stored_keys(&MemoryStore::default()).unwrap().is_none());
    }

    #[test]
    fn missing_and_different_keys_are_reported() {
        let store = MemoryStore::default();
        let identity = create(&store, 1).unwrap();
        let elsewhere = create(&MemoryStore::default(), 1).unwrap();
        assert!(matches!(
            learner_keys(&store, &elsewhere),
            Err(LearnerKeyError::WrongKey)
        ));
        assert!(matches!(
            learner_keys(&MemoryStore::default(), &identity),
            Err(LearnerKeyError::Missing)
        ));
        store.save(ACCOUNT, &[1, 2, 3]).unwrap();
        assert!(matches!(
            learner_keys(&store, &identity),
            Err(LearnerKeyError::WrongKey)
        ));
    }

    #[test]
    fn a_backup_restores_the_key_on_another_computer() {
        let here = MemoryStore::default();
        let identity = create(&here, 1).unwrap();
        assert!(matches!(
            export_backup(&here, &identity, "short"),
            Err(LearnerKeyError::WeakPassphrase)
        ));
        let backup = export_backup(&here, &identity, PASSPHRASE).unwrap();
        let text = String::from_utf8(backup.clone()).unwrap();
        let seed = load_seed(&here).unwrap().unwrap();
        assert!(!text.contains(&passphrase_hex(seed.as_ref())));
        assert!(text.contains(&identity.signing_key));

        let there = MemoryStore::default();
        assert!(matches!(
            restore_backup(&there, &backup, "correct horse battery!"),
            Err(LearnerKeyError::WrongPassphrase)
        ));
        assert_eq!(
            restore_backup(&there, &backup, PASSPHRASE).unwrap(),
            identity
        );
        assert!(learner_keys(&there, &identity).is_ok());
        // Restoring the same key again is harmless.
        assert_eq!(
            restore_backup(&there, &backup, PASSPHRASE).unwrap(),
            identity
        );

        let other = MemoryStore::default();
        create(&other, 1).unwrap();
        assert!(matches!(
            restore_backup(&other, &backup, PASSPHRASE),
            Err(LearnerKeyError::Exists)
        ));
    }

    #[test]
    fn tampered_and_foreign_backups_are_refused() {
        let store = MemoryStore::default();
        let identity = create(&store, 1).unwrap();
        let backup = export_backup(&store, &identity, PASSPHRASE).unwrap();
        let target = MemoryStore::default();

        let mut swapped: serde_json::Value = serde_json::from_slice(&backup).unwrap();
        let other = create(&MemoryStore::default(), 1).unwrap();
        swapped["identity"] = serde_json::to_value(&other).unwrap();
        assert!(matches!(
            restore_backup(&target, &serde_json::to_vec(&swapped).unwrap(), PASSPHRASE),
            Err(LearnerKeyError::WrongPassphrase)
        ));
        let mut publisher: serde_json::Value = serde_json::from_slice(&backup).unwrap();
        publisher["format"] = "zeron.publisher-key-backup".into();
        assert!(matches!(
            restore_backup(
                &target,
                &serde_json::to_vec(&publisher).unwrap(),
                PASSPHRASE
            ),
            Err(LearnerKeyError::InvalidBackup)
        ));
        assert!(matches!(
            restore_backup(&target, &vec![b' '; MAX_BACKUP_BYTES + 1], PASSPHRASE),
            Err(LearnerKeyError::InvalidBackup)
        ));
        assert!(target.load(ACCOUNT).unwrap().is_none());
    }
}
