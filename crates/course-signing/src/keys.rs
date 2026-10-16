//! Ed25519 keys in Zeron's text form.

use std::{fmt, str::FromStr};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::SigningError;

const KEY_PREFIX: &str = "ed25519-";

/// A publisher's public key, written `ed25519-<64 lowercase hex>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublicKey([u8; 32]);

/// A publisher's secret signing key. It is zeroized on drop and never
/// printed.
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl PublicKey {
    /// The raw key bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Verify `signature` (128 lowercase hex characters) over `message`
    /// with strict Ed25519 rules.
    pub(crate) fn verify(self, message: &[u8], signature: &str) -> bool {
        let Some(bytes) = decode_hex::<64>(signature) else {
            return false;
        };
        let Ok(key) = VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        key.verify_strict(message, &Signature::from_bytes(&bytes))
            .is_ok()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(KEY_PREFIX)?;
        formatter.write_str(&encode_hex(&self.0))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl FromStr for PublicKey {
    type Err = SigningError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text
            .strip_prefix(KEY_PREFIX)
            .and_then(decode_hex::<32>)
            .ok_or(SigningError::InvalidKey)?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| SigningError::InvalidKey)?;
        Ok(Self(bytes))
    }
}

impl SecretKey {
    /// A key from 32 secret bytes, for example fresh output of the OS random
    /// source or a restored backup.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(bytes))
    }

    /// The secret bytes, for the credential store or an encrypted backup.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// The matching public key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key().to_bytes())
    }

    /// A signature over `message` as 128 lowercase hex characters.
    pub(crate) fn sign(&self, message: &[u8]) -> String {
        encode_hex(&self.0.sign(message).to_bytes())
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SecretKey({})", self.public_key())
    }
}

/// A short form for people to compare out of band, such as
/// `7F3A-91C2-0B44-E8D1`: the first 8 bytes of the key's SHA-256.
#[must_use]
pub fn fingerprint(key: &PublicKey) -> String {
    let digest = Sha256::digest(key.0);
    digest[..8]
        .chunks(2)
        .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut bytes = [0_u8; N];
    for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        };
        *byte = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_through_text_and_reject_lookalikes() {
        let secret = SecretKey::from_bytes(&[7; 32]);
        let public = secret.public_key();
        let text = public.to_string();
        assert!(text.starts_with("ed25519-"));
        assert_eq!(text.len(), 8 + 64);
        assert_eq!(text.parse::<PublicKey>().unwrap(), public);
        assert_eq!(
            text.to_uppercase().parse::<PublicKey>(),
            Err(SigningError::InvalidKey)
        );
        assert!("ed25519-00".parse::<PublicKey>().is_err());
        assert!(
            text.replace("ed25519-", "rsa-")
                .parse::<PublicKey>()
                .is_err()
        );
        assert!(!format!("{secret:?}").contains(&encode_hex(&secret.to_bytes())));
        assert_eq!(
            SecretKey::from_bytes(&secret.to_bytes()).public_key(),
            public
        );
    }

    #[test]
    fn signatures_verify_only_for_their_key_and_message() {
        let secret = SecretKey::from_bytes(&[1; 32]);
        let other = SecretKey::from_bytes(&[2; 32]);
        let signature = secret.sign(b"message");
        assert_eq!(signature.len(), 128);
        assert!(secret.public_key().verify(b"message", &signature));
        assert!(!secret.public_key().verify(b"messagE", &signature));
        assert!(!other.public_key().verify(b"message", &signature));
        assert!(!secret.public_key().verify(b"message", "00"));
        assert!(
            !secret
                .public_key()
                .verify(b"message", &signature.to_uppercase())
        );
    }

    #[test]
    fn fingerprints_are_short_and_stable() {
        let key = SecretKey::from_bytes(&[3; 32]).public_key();
        let print = fingerprint(&key);
        assert_eq!(print.len(), 19);
        assert_eq!(print, fingerprint(&key));
        assert_eq!(print.split('-').count(), 4);
    }
}
