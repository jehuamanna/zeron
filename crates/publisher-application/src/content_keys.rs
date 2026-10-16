//! Release content keys for paid courses (ADR 0017 decision 2).
//!
//! Each paid course version is sealed under its own random 256-bit content
//! key. The key lives only in the publisher's credential store, beside the
//! signing key, under the account `content-key:<courseId>@<version>`: never
//! in SQLite, never over IPC, never in logs. A failed publishing attempt and
//! its retry reuse the same key, because a course version's content is fixed.

use course_access::ContentKey;
use publisher_identity::{CredentialStore, IdentityError};

/// Credential store account holding one course version's content key.
#[must_use]
pub fn content_key_account(course_id: &str, version: &str) -> String {
    format!("content-key:{course_id}@{version}")
}

/// The stored content key of a course version, if this computer has one.
///
/// # Errors
/// Returns the store's failure, or [`IdentityError::WrongKey`] when the
/// stored entry is not a 32-byte key.
pub fn stored_content_key(
    store: &dyn CredentialStore,
    course_id: &str,
    version: &str,
) -> Result<Option<ContentKey>, IdentityError> {
    let Some(secret) = store.load(&content_key_account(course_id, version))? else {
        return Ok(None);
    };
    let bytes: [u8; 32] = secret
        .as_slice()
        .try_into()
        .map_err(|_| IdentityError::WrongKey)?;
    Ok(Some(ContentKey::from_bytes(bytes)))
}

/// The content key to seal a course version with: the stored one, or a new
/// random key saved to the store first.
///
/// # Errors
/// Returns the store's or the random source's failure.
pub fn release_content_key(
    store: &dyn CredentialStore,
    course_id: &str,
    version: &str,
) -> Result<ContentKey, IdentityError> {
    if let Some(key) = stored_content_key(store, course_id, version)? {
        return Ok(key);
    }
    let mut bytes = zeroize::Zeroizing::new([0_u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|error| IdentityError::Random(error.to_string()))?;
    store.save(&content_key_account(course_id, version), bytes.as_ref())?;
    Ok(ContentKey::from_bytes(*bytes))
}

#[cfg(test)]
mod tests {
    use publisher_identity::MemoryStore;

    use super::*;

    #[test]
    fn a_course_version_keeps_one_key() {
        let store = MemoryStore::default();
        assert!(
            stored_content_key(&store, "course_a", "1.0.0")
                .unwrap()
                .is_none()
        );
        let first = release_content_key(&store, "course_a", "1.0.0").unwrap();
        let again = release_content_key(&store, "course_a", "1.0.0").unwrap();
        assert_eq!(first, again);
        assert_eq!(
            stored_content_key(&store, "course_a", "1.0.0").unwrap(),
            Some(first.clone())
        );
        let next = release_content_key(&store, "course_a", "1.0.1").unwrap();
        assert_ne!(first, next);
    }

    #[test]
    fn a_malformed_entry_is_refused() {
        let store = MemoryStore::default();
        store
            .save(&content_key_account("c", "1.0.0"), &[1; 31])
            .unwrap();
        assert!(matches!(
            stored_content_key(&store, "c", "1.0.0"),
            Err(IdentityError::WrongKey)
        ));
    }
}
