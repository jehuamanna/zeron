//! Encrypted course resources (ADR 0017 decision 2).
//!
//! Format `zeron.chunked-xchacha20poly1305.v1`:
//!
//! - every paid resource is sealed under its own key, derived with
//!   HKDF-SHA256 from the release's 256-bit content key and the resource ID;
//! - the plaintext is split into chunks of [`PLAINTEXT_CHUNK_BYTES`]; chunk
//!   `i` is sealed with XChaCha20-Poly1305 under nonce `i` and associated data
//!   binding the resource ID, `i`, and whether it is the final chunk;
//! - the ciphertext is the sealed chunks back to back, so each sealed chunk
//!   is exactly 64 KiB and sits inside one torrent piece or covers whole
//!   pieces. There is no header: the manifest carries the scheme, chunk
//!   size, and plaintext length.
//!
//! The crate performs no file or network I/O of its own. It works over
//! caller-supplied readers and writers, and never returns bytes from a chunk
//! that failed authentication.

mod reader;

use std::io::{self, Read, Write};
use std::sync::Arc;

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use reader::DecryptingReader;

/// Manifest `encryption.scheme` of this format.
pub const ENCRYPTION_SCHEME: &str = "zeron.chunked-xchacha20poly1305.v1";
/// Plaintext bytes per chunk; every chunk but the last is exactly this long.
pub const PLAINTEXT_CHUNK_BYTES: u32 = 65_520;
/// Authentication tag appended to every sealed chunk.
pub const TAG_BYTES: u32 = 16;
/// Bytes of one full sealed chunk (64 KiB).
pub const SEALED_CHUNK_BYTES: u32 = PLAINTEXT_CHUNK_BYTES + TAG_BYTES;

const KEY_SALT: &[u8] = b"zeron.content.v1";
const AAD_PREFIX: &[u8] = b"zeron.chunk.v1\n";

/// Why encrypted content could not be produced or read.
#[derive(Debug, Error)]
pub enum AccessError {
    /// A sealed chunk failed authentication: wrong key, or altered bytes.
    #[error("encrypted content failed authentication")]
    Authentication,
    /// The ciphertext length does not match the declared plaintext length.
    #[error("encrypted content has the wrong length")]
    Length,
    /// Reading or writing failed.
    #[error("encrypted content could not be read or written")]
    Io(#[from] io::Error),
}

impl From<AccessError> for io::Error {
    fn from(error: AccessError) -> Self {
        match error {
            AccessError::Io(error) => error,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}

/// A release's 256-bit content key. Zeroized on drop and never printed.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct ContentKey([u8; 32]);

impl ContentKey {
    /// Wrap 32 bytes from a cryptographically secure source.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw key, for storage behind a Rust boundary.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for ContentKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ContentKey(..)")
    }
}

/// Sealed length of a resource with `plaintext_length` bytes.
#[must_use]
pub fn sealed_length(plaintext_length: u64) -> u64 {
    plaintext_length + chunk_count(plaintext_length) * u64::from(TAG_BYTES)
}

fn chunk_count(plaintext_length: u64) -> u64 {
    plaintext_length
        .div_ceil(u64::from(PLAINTEXT_CHUNK_BYTES))
        .max(1)
}

/// Cipher state for one resource, shared by all its readers.
#[derive(Clone)]
pub struct ResourceCipher {
    cipher: Arc<XChaCha20Poly1305>,
    resource_id: Arc<str>,
    plaintext_length: u64,
}

impl std::fmt::Debug for ResourceCipher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResourceCipher")
            .field("resource_id", &self.resource_id)
            .field("plaintext_length", &self.plaintext_length)
            .finish_non_exhaustive()
    }
}

impl ResourceCipher {
    /// Derive the cipher for `resource_id` of a release sealed under `key`.
    #[must_use]
    pub fn new(key: &ContentKey, resource_id: &str, plaintext_length: u64) -> Self {
        let mut file_key = [0_u8; 32];
        Hkdf::<Sha256>::new(Some(KEY_SALT), &key.0)
            .expand(resource_id.as_bytes(), &mut file_key)
            .unwrap_or_else(|_| unreachable!("32 bytes is a valid HKDF-SHA256 output length"));
        let cipher = XChaCha20Poly1305::new(&file_key.into());
        file_key.zeroize();
        Self {
            cipher: Arc::new(cipher),
            resource_id: resource_id.into(),
            plaintext_length,
        }
    }

    /// Declared plaintext length.
    #[must_use]
    pub const fn plaintext_length(&self) -> u64 {
        self.plaintext_length
    }

    /// Sealed length on disk and in the torrent.
    #[must_use]
    pub fn sealed_length(&self) -> u64 {
        sealed_length(self.plaintext_length)
    }

    fn chunk_count(&self) -> u64 {
        chunk_count(self.plaintext_length)
    }

    fn plaintext_chunk_length(&self, index: u64) -> usize {
        let start = index * u64::from(PLAINTEXT_CHUNK_BYTES);
        let length = self
            .plaintext_length
            .saturating_sub(start)
            .min(u64::from(PLAINTEXT_CHUNK_BYTES));
        usize::try_from(length).unwrap_or_else(|_| unreachable!("a chunk fits in usize"))
    }

    fn nonce(index: u64) -> XNonce {
        let mut nonce = [0_u8; 24];
        nonce[..8].copy_from_slice(&index.to_le_bytes());
        nonce.into()
    }

    fn aad(&self, index: u64) -> Vec<u8> {
        let id = self.resource_id.as_bytes();
        let id_length =
            u32::try_from(id.len()).unwrap_or_else(|_| unreachable!("resource IDs are short"));
        let mut aad = Vec::with_capacity(AAD_PREFIX.len() + 4 + id.len() + 9);
        aad.extend_from_slice(AAD_PREFIX);
        aad.extend_from_slice(&id_length.to_be_bytes());
        aad.extend_from_slice(id);
        aad.extend_from_slice(&index.to_be_bytes());
        aad.push(u8::from(index + 1 == self.chunk_count()));
        aad
    }

    fn seal_chunk(&self, index: u64, plaintext: &[u8]) -> Vec<u8> {
        let aad = self.aad(index);
        self.cipher
            .encrypt(
                &Self::nonce(index),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .unwrap_or_else(|_| unreachable!("sealing a 64 KiB chunk cannot fail"))
    }

    fn open_chunk(&self, index: u64, sealed: &[u8]) -> Result<Vec<u8>, AccessError> {
        let aad = self.aad(index);
        self.cipher
            .decrypt(
                &Self::nonce(index),
                Payload {
                    msg: sealed,
                    aad: &aad,
                },
            )
            .map_err(|_| AccessError::Authentication)
    }

    /// Byte offset and length of sealed chunk `index`.
    fn sealed_span(&self, index: u64) -> (u64, usize) {
        (
            index * u64::from(SEALED_CHUNK_BYTES),
            self.plaintext_chunk_length(index) + TAG_BYTES as usize,
        )
    }

    /// Seal exactly `plaintext_length` bytes from `source` into `sink`.
    /// Returns the sealed length.
    ///
    /// # Errors
    ///
    /// Fails when the source is shorter than declared or I/O fails. A source
    /// longer than declared is refused too, so a file that grew during
    /// packaging is never sealed partially.
    pub fn seal(&self, source: &mut impl Read, sink: &mut impl Write) -> Result<u64, AccessError> {
        let mut buffer = vec![0_u8; PLAINTEXT_CHUNK_BYTES as usize];
        let mut written = 0;
        for index in 0..self.chunk_count() {
            let length = self.plaintext_chunk_length(index);
            source.read_exact(&mut buffer[..length]).map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    AccessError::Length
                } else {
                    AccessError::Io(error)
                }
            })?;
            let sealed = self.seal_chunk(index, &buffer[..length]);
            sink.write_all(&sealed)?;
            written += sealed.len() as u64;
        }
        buffer.zeroize();
        if source.read(&mut [0_u8; 1])? != 0 {
            return Err(AccessError::Length);
        }
        Ok(written)
    }

    /// Open a whole sealed resource held in memory, such as a transcript.
    ///
    /// # Errors
    ///
    /// Fails on a wrong length or any chunk that fails authentication.
    pub fn open_all(&self, sealed: &[u8]) -> Result<Vec<u8>, AccessError> {
        if sealed.len() as u64 != self.sealed_length() {
            return Err(AccessError::Length);
        }
        let mut plaintext = Vec::with_capacity(
            usize::try_from(self.plaintext_length).map_err(|_| AccessError::Length)?,
        );
        for index in 0..self.chunk_count() {
            let (offset, length) = self.sealed_span(index);
            let offset = usize::try_from(offset).map_err(|_| AccessError::Length)?;
            plaintext.extend_from_slice(&self.open_chunk(index, &sealed[offset..offset + length])?);
        }
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    pub(crate) fn fixture(length: usize) -> Vec<u8> {
        (0..length)
            .map(|i| u8::try_from(i * 31 % 251).unwrap())
            .collect()
    }

    pub(crate) fn sealed(plaintext: &[u8], id: &str) -> (ResourceCipher, Vec<u8>) {
        let cipher =
            ResourceCipher::new(&ContentKey::from_bytes([7; 32]), id, plaintext.len() as u64);
        let mut out = Vec::new();
        let written = cipher.seal(&mut Cursor::new(plaintext), &mut out).unwrap();
        assert_eq!(written, out.len() as u64);
        assert_eq!(out.len() as u64, cipher.sealed_length());
        (cipher, out)
    }

    #[test]
    fn sealed_chunks_are_64_kib() {
        assert_eq!(SEALED_CHUNK_BYTES, 64 * 1024);
        assert_eq!(sealed_length(0), 16);
        assert_eq!(sealed_length(1), 17);
        assert_eq!(sealed_length(65_520), 65_536);
        assert_eq!(sealed_length(65_521), 65_536 + 17);
    }

    #[test]
    fn format_constants_match_the_manifest_contract() {
        assert_eq!(ENCRYPTION_SCHEME, course_format::ENCRYPTION_SCHEME);
        assert_eq!(PLAINTEXT_CHUNK_BYTES, course_format::ENCRYPTION_CHUNK_SIZE);
        assert_eq!(TAG_BYTES, course_format::ENCRYPTION_TAG_SIZE);
        for length in [0, 1, 65_519, 65_520, 65_521, 10_000_000] {
            assert_eq!(
                sealed_length(length),
                course_format::sealed_byte_length(length)
            );
        }
    }

    #[test]
    fn whole_resources_round_trip() {
        let chunk = PLAINTEXT_CHUNK_BYTES as usize;
        for length in [0, 1, chunk - 1, chunk, chunk + 1, 3 * chunk + 17] {
            let plaintext = fixture(length);
            let (cipher, sealed) = sealed(&plaintext, "r1");
            assert_eq!(cipher.open_all(&sealed).unwrap(), plaintext);
        }
    }

    #[test]
    fn known_answer_is_stable() {
        // Guards the wire format: key derivation, nonce, AAD, and layout.
        let (_, sealed) = sealed(b"zeron", "lesson-1-video");
        let hex = sealed.iter().fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        assert_eq!(hex, KNOWN_ANSWER);
    }

    // Computed independently with a pure-Python XChaCha20-Poly1305 and HKDF
    // (checked against draft-irtf-cfrg-xchacha test vector A.3.1).
    const KNOWN_ANSWER: &str = "a6fba633cb758cbbb2b801b431cf9cf6ce78a3eb8c";

    #[test]
    fn sources_of_the_wrong_length_are_refused() {
        let cipher = ResourceCipher::new(&ContentKey::from_bytes([7; 32]), "r", 10);
        let mut out = Vec::new();
        assert!(matches!(
            cipher.seal(&mut Cursor::new(vec![0; 9]), &mut out),
            Err(AccessError::Length)
        ));
        assert!(matches!(
            cipher.seal(&mut Cursor::new(vec![0; 11]), &mut out),
            Err(AccessError::Length)
        ));
    }

    #[test]
    fn tampering_is_refused() {
        let plaintext = fixture(3 * PLAINTEXT_CHUNK_BYTES as usize + 100);
        let (cipher, sealed_bytes) = sealed(&plaintext, "r1");
        let step = SEALED_CHUNK_BYTES as usize;

        let mut flipped = sealed_bytes.clone();
        flipped[step + 5] ^= 1;
        assert!(matches!(
            cipher.open_all(&flipped),
            Err(AccessError::Authentication)
        ));

        let mut swapped = sealed_bytes.clone();
        swapped[..step].copy_from_slice(&sealed_bytes[step..2 * step]);
        swapped[step..2 * step].copy_from_slice(&sealed_bytes[..step]);
        assert!(matches!(
            cipher.open_all(&swapped),
            Err(AccessError::Authentication)
        ));

        let (_, other) = sealed(&plaintext, "r2");
        let mut spliced = sealed_bytes.clone();
        spliced[..step].copy_from_slice(&other[..step]);
        assert!(matches!(
            cipher.open_all(&spliced),
            Err(AccessError::Authentication)
        ));

        // Cut at a chunk boundary and declared shorter: the new last chunk
        // was sealed as non-final.
        let short = ResourceCipher::new(
            &ContentKey::from_bytes([7; 32]),
            "r1",
            2 * u64::from(PLAINTEXT_CHUNK_BYTES),
        );
        assert!(matches!(
            short.open_all(&sealed_bytes[..2 * step]),
            Err(AccessError::Authentication)
        ));

        let wrong = ResourceCipher::new(
            &ContentKey::from_bytes([8; 32]),
            "r1",
            plaintext.len() as u64,
        );
        assert!(matches!(
            wrong.open_all(&sealed_bytes),
            Err(AccessError::Authentication)
        ));
        assert!(matches!(
            cipher.open_all(&sealed_bytes[1..]),
            Err(AccessError::Length)
        ));
    }

    #[test]
    fn keys_never_print() {
        assert_eq!(
            format!("{:?}", ContentKey::from_bytes([9; 32])),
            "ContentKey(..)"
        );
        let cipher = ResourceCipher::new(&ContentKey::from_bytes([9; 32]), "r", 1);
        assert!(!format!("{cipher:?}").contains("09"));
    }
}
