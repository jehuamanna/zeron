//! Sealing a release's content key to one learner (ADR 0017 decision 4).
//!
//! HPKE (RFC 9180) base mode with DHKEM(X25519, HKDF-SHA256), HKDF-SHA256,
//! and ChaCha20-Poly1305, through the `hpke` crate. `info` is
//! `zeron.license.v1`; the associated data binds the course ID and version,
//! so a wrapped key cannot be moved to another course or version. The
//! wrapped form is lowercase hex of the 32-byte encapsulated key followed by
//! the 48-byte sealed content key.

use hpke::{Deserializable, OpModeR, OpModeS, Serializable, kem::X25519HkdfSha256};
use zeroize::Zeroizing;

use crate::{AccessError, ContentKey};

type Kem = X25519HkdfSha256;
type Kdf = hpke::kdf::HkdfSha256;
type Cipher = hpke::aead::ChaCha20Poly1305;

const INFO: &[u8] = b"zeron.license.v1";
const ENCAPSULATED_BYTES: usize = 32;
const SEALED_KEY_BYTES: usize = 48;

fn associated_data(course_id: &str, course_version: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(course_id.len() + course_version.len() + 1);
    data.extend_from_slice(course_id.as_bytes());
    data.push(b'\n');
    data.extend_from_slice(course_version.as_bytes());
    data
}

/// Seal `key` to the learner whose X25519 public key is `recipient`.
///
/// # Errors
/// Returns [`AccessError::Authentication`] when `recipient` is not a usable
/// X25519 public key.
pub fn wrap_content_key(
    key: &ContentKey,
    recipient: &[u8; 32],
    course_id: &str,
    course_version: &str,
) -> Result<String, AccessError> {
    let recipient = <Kem as hpke::Kem>::PublicKey::from_bytes(recipient)
        .map_err(|_| AccessError::Authentication)?;
    let (encapsulated, sealed) = hpke::single_shot_seal::<Cipher, Kdf, Kem>(
        &OpModeS::Base,
        &recipient,
        INFO,
        key.as_bytes(),
        &associated_data(course_id, course_version),
    )
    .map_err(|_| AccessError::Authentication)?;
    let mut bytes = encapsulated.to_bytes().to_vec();
    bytes.extend_from_slice(&sealed);
    Ok(hex(&bytes))
}

/// Open a wrapped content key with the learner's X25519 secret key.
///
/// # Errors
/// Returns [`AccessError::Length`] for a malformed value and
/// [`AccessError::Authentication`] for another learner, course, or version,
/// or altered bytes.
pub fn unwrap_content_key(
    wrapped: &str,
    secret: &[u8; 32],
    course_id: &str,
    course_version: &str,
) -> Result<ContentKey, AccessError> {
    let bytes = unhex(wrapped)
        .filter(|bytes| bytes.len() == ENCAPSULATED_BYTES + SEALED_KEY_BYTES)
        .ok_or(AccessError::Length)?;
    let (encapsulated, sealed) = bytes.split_at(ENCAPSULATED_BYTES);
    let encapsulated = <Kem as hpke::Kem>::EncappedKey::from_bytes(encapsulated)
        .map_err(|_| AccessError::Length)?;
    let secret = <Kem as hpke::Kem>::PrivateKey::from_bytes(secret)
        .map_err(|_| AccessError::Authentication)?;
    let opened = Zeroizing::new(
        hpke::single_shot_open::<Cipher, Kdf, Kem>(
            &OpModeR::Base,
            &secret,
            &encapsulated,
            INFO,
            sealed,
            &associated_data(course_id, course_version),
        )
        .map_err(|_| AccessError::Authentication)?,
    );
    let key: [u8; 32] = opened
        .as_slice()
        .try_into()
        .map_err(|_| AccessError::Length)?;
    Ok(ContentKey::from_bytes(key))
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
    use hpke::Kem as _;

    use super::*;

    fn learner(seed: u8) -> ([u8; 32], [u8; 32]) {
        let (secret, public) = Kem::derive_keypair(&[seed; 32]);
        (secret.to_bytes().into(), public.to_bytes().into())
    }

    #[test]
    fn a_wrapped_key_opens_only_for_its_learner_course_and_version() {
        let key = ContentKey::from_bytes([42; 32]);
        let (secret, public) = learner(1);
        let wrapped = wrap_content_key(&key, &public, "course_a", "1.0.0").unwrap();
        assert_eq!(wrapped.len(), 2 * (ENCAPSULATED_BYTES + SEALED_KEY_BYTES));
        assert!(!wrapped.contains(&hex(key.as_bytes())));
        assert_eq!(
            unwrap_content_key(&wrapped, &secret, "course_a", "1.0.0").unwrap(),
            key
        );
        let (other, _) = learner(2);
        for (secret, course, version) in [
            (other, "course_a", "1.0.0"),
            (secret, "course_b", "1.0.0"),
            (secret, "course_a", "1.0.1"),
        ] {
            assert!(matches!(
                unwrap_content_key(&wrapped, &secret, course, version),
                Err(AccessError::Authentication)
            ));
        }
        let mut altered = wrapped.clone().into_bytes();
        altered[100] = if altered[100] == b'0' { b'1' } else { b'0' };
        assert!(matches!(
            unwrap_content_key(
                &String::from_utf8(altered).unwrap(),
                &secret,
                "course_a",
                "1.0.0"
            ),
            Err(AccessError::Authentication)
        ));
        assert!(matches!(
            unwrap_content_key(&wrapped[2..], &secret, "course_a", "1.0.0"),
            Err(AccessError::Length)
        ));
        assert!(matches!(
            unwrap_content_key("zz", &secret, "course_a", "1.0.0"),
            Err(AccessError::Length)
        ));
    }

    #[test]
    fn wrapping_is_randomized() {
        let key = ContentKey::from_bytes([42; 32]);
        let (_, public) = learner(1);
        assert_ne!(
            wrap_content_key(&key, &public, "c", "1").unwrap(),
            wrap_content_key(&key, &public, "c", "1").unwrap()
        );
    }
}
