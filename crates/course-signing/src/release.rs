//! The `zeron-signature.json` file that signs a release package.

use serde::{Deserialize, Serialize};

use crate::{
    PublicKey, SecretKey, SigningError,
    statements::{
        MAX_ROTATIONS, RevocationStatement, RotationStatement, Signed, canonical, revoked,
        verify_chain,
    },
};

/// Name of the signature file at the package root, beside `course.json`.
pub const RELEASE_SIGNATURE_FILE: &str = "zeron-signature.json";
/// The `format` value every release signature carries.
pub const RELEASE_SIGNATURE_FORMAT: &str = "zeron.release-signature";
/// Largest release signature file read.
pub const MAX_RELEASE_SIGNATURE_BYTES: usize = 64 * 1024;
const VERSION: u32 = 1;
const DOMAIN: &[u8] = b"zeron.release-signature.v1\n";

/// What a publisher signs for a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseStatement {
    /// The manifest's `courseId`.
    pub course_id: String,
    /// The manifest's `version`.
    pub course_version: String,
    /// Lowercase hex SHA-256 of the exact `course.json` bytes.
    pub manifest_sha256: String,
    /// The key that signed, the last of the rotation chain.
    pub publisher_key: String,
    /// Unix seconds.
    pub signed_at: u64,
}

/// The contents of `zeron-signature.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseSignature {
    /// Always [`RELEASE_SIGNATURE_FORMAT`].
    pub format: String,
    /// Always 1.
    pub version: u32,
    /// The signed statement.
    pub statement: ReleaseStatement,
    /// Signature by `statement.publisher_key`.
    pub signature: String,
    /// Rotations from the publisher's root key to the signing key.
    pub rotations: Vec<Signed<RotationStatement>>,
}

/// A release signature that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRelease {
    /// The publisher's identity, the root of the chain.
    pub root: PublicKey,
    /// The key that signed this release.
    pub signing_key: PublicKey,
    /// When it was signed, in Unix seconds.
    pub signed_at: u64,
    /// The reason, when the signing key was revoked at the signing time.
    pub revoked: Option<String>,
}

impl ReleaseSignature {
    /// The file as written into a package: pretty JSON with a final newline.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

/// Sign a release with the publisher's current key. `rotations` is the chain
/// from the root key to `secret`'s key, empty when it is the root.
#[must_use]
pub fn sign_release(
    secret: &SecretKey,
    course_id: &str,
    course_version: &str,
    manifest_sha256: &str,
    signed_at: u64,
    rotations: Vec<Signed<RotationStatement>>,
) -> ReleaseSignature {
    let statement = ReleaseStatement {
        course_id: course_id.to_owned(),
        course_version: course_version.to_owned(),
        manifest_sha256: manifest_sha256.to_owned(),
        publisher_key: secret.public_key().to_string(),
        signed_at,
    };
    ReleaseSignature {
        format: RELEASE_SIGNATURE_FORMAT.to_owned(),
        version: VERSION,
        signature: secret.sign(&canonical(DOMAIN, &statement)),
        statement,
        rotations,
    }
}

/// Parse an untrusted `zeron-signature.json`.
///
/// # Errors
/// Returns [`SigningError::TooLarge`] or [`SigningError::Malformed`].
pub fn parse_release_signature(bytes: &[u8]) -> Result<ReleaseSignature, SigningError> {
    if bytes.len() > MAX_RELEASE_SIGNATURE_BYTES {
        return Err(SigningError::TooLarge("release signature"));
    }
    let signature: ReleaseSignature =
        serde_json::from_slice(bytes).map_err(|_| SigningError::Malformed("release signature"))?;
    if signature.format != RELEASE_SIGNATURE_FORMAT
        || signature.version != VERSION
        || signature.rotations.len() > MAX_ROTATIONS
    {
        return Err(SigningError::Malformed("release signature"));
    }
    Ok(signature)
}

/// Verify a release signature against the package it came with and the
/// publisher identity its `course.json` names (`publisherId`, the root key).
/// The chain must lead from that root to the signing key, and no rotation in
/// it may be made by a key revoked by then. A signing key revoked at the
/// signing time still verifies, with the reason in
/// [`VerifiedRelease::revoked`], so callers can say why they refuse it.
///
/// # Errors
/// Returns [`SigningError::Mismatch`] when the statement describes another
/// course, version, or manifest, [`SigningError::BrokenChain`],
/// [`SigningError::BadSignature`], or [`SigningError::InvalidKey`].
pub fn verify_release(
    signature: &ReleaseSignature,
    course_id: &str,
    course_version: &str,
    manifest_sha256: &str,
    publisher_id: &str,
    revocations: &[Signed<RevocationStatement>],
) -> Result<VerifiedRelease, SigningError> {
    let statement = &signature.statement;
    if statement.course_id != course_id || statement.course_version != course_version {
        return Err(SigningError::Mismatch("another course or version"));
    }
    if statement.manifest_sha256 != manifest_sha256 {
        return Err(SigningError::Mismatch("another course.json"));
    }
    let root: PublicKey = publisher_id.parse()?;
    let signing_key: PublicKey = statement.publisher_key.parse()?;
    let chain = verify_chain(&root, &signature.rotations)?;
    if chain.last() != Some(&signing_key) {
        return Err(SigningError::BrokenChain);
    }
    for (index, rotation) in signature.rotations.iter().enumerate() {
        if revoked(
            &chain,
            revocations,
            &chain[index],
            rotation.statement.rotated_at,
        )
        .is_some()
        {
            return Err(SigningError::BrokenChain);
        }
    }
    if !signing_key.verify(&canonical(DOMAIN, statement), &signature.signature) {
        return Err(SigningError::BadSignature("release"));
    }
    Ok(VerifiedRelease {
        root,
        signing_key,
        signed_at: statement.signed_at,
        revoked: revoked(&chain, revocations, &signing_key, statement.signed_at)
            .map(|revocation| revocation.reason.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{revoke, rotate};

    const SHA: &str = "ab12";

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn a_signed_release_round_trips_and_verifies() {
        let root = key(1);
        let signed = sign_release(&root, "course_a", "1.0.0", SHA, 1_000, Vec::new());
        let parsed = parse_release_signature(&signed.to_bytes()).unwrap();
        assert_eq!(parsed, signed);
        let verified = verify_release(
            &parsed,
            "course_a",
            "1.0.0",
            SHA,
            &root.public_key().to_string(),
            &[],
        )
        .unwrap();
        assert_eq!(verified.root, root.public_key());
        assert_eq!(verified.signing_key, root.public_key());
        assert_eq!(verified.signed_at, 1_000);
        assert_eq!(verified.revoked, None);
        let revocation = revoke(&root, &root.public_key(), 900, "key leaked");
        let flagged = verify_release(
            &parsed,
            "course_a",
            "1.0.0",
            SHA,
            &root.public_key().to_string(),
            &[revocation],
        )
        .unwrap();
        assert_eq!(flagged.revoked.as_deref(), Some("key leaked"));
    }

    #[test]
    fn a_rotated_key_signs_for_the_same_identity() {
        let (root, next) = (key(1), key(2));
        let chain = vec![rotate(&root, &next.public_key(), 500)];
        let signed = sign_release(&next, "course_a", "2.0.0", SHA, 1_000, chain);
        let verified = verify_release(
            &signed,
            "course_a",
            "2.0.0",
            SHA,
            &root.public_key().to_string(),
            &[],
        )
        .unwrap();
        assert_eq!(verified.root, root.public_key());
        assert_eq!(verified.signing_key, next.public_key());
        let without_chain = ReleaseSignature {
            rotations: Vec::new(),
            ..signed.clone()
        };
        assert_eq!(
            verify_release(
                &without_chain,
                "course_a",
                "2.0.0",
                SHA,
                &root.public_key().to_string(),
                &[]
            ),
            Err(SigningError::BrokenChain)
        );
        let revoked_root = revoke(&root, &root.public_key(), 400, "stolen");
        assert_eq!(
            verify_release(
                &signed,
                "course_a",
                "2.0.0",
                SHA,
                &root.public_key().to_string(),
                &[revoked_root]
            ),
            Err(SigningError::BrokenChain)
        );
    }

    #[test]
    fn signatures_for_anything_else_are_refused() {
        let (root, impostor) = (key(1), key(9));
        let root_id = root.public_key().to_string();
        let signed = sign_release(&root, "course_a", "1.0.0", SHA, 1, Vec::new());
        assert!(matches!(
            verify_release(&signed, "course_b", "1.0.0", SHA, &root_id, &[]),
            Err(SigningError::Mismatch(_))
        ));
        assert!(matches!(
            verify_release(&signed, "course_a", "1.0.1", SHA, &root_id, &[]),
            Err(SigningError::Mismatch(_))
        ));
        assert!(matches!(
            verify_release(&signed, "course_a", "1.0.0", "ffff", &root_id, &[]),
            Err(SigningError::Mismatch(_))
        ));
        let by_impostor = sign_release(&impostor, "course_a", "1.0.0", SHA, 1, Vec::new());
        assert_eq!(
            verify_release(&by_impostor, "course_a", "1.0.0", SHA, &root_id, &[]),
            Err(SigningError::BrokenChain)
        );
        let mut tampered = signed.clone();
        tampered.statement.signed_at = 2;
        assert_eq!(
            verify_release(&tampered, "course_a", "1.0.0", SHA, &root_id, &[]),
            Err(SigningError::BadSignature("release"))
        );
        assert_eq!(
            verify_release(&signed, "course_a", "1.0.0", SHA, "publisher_plain", &[]),
            Err(SigningError::InvalidKey)
        );
    }

    #[test]
    fn untrusted_files_are_bounded_and_strict() {
        let signed = sign_release(&key(1), "course_a", "1.0.0", SHA, 1, Vec::new());
        let mut value: serde_json::Value = serde_json::from_slice(&signed.to_bytes()).unwrap();
        value["extra"] = true.into();
        assert!(parse_release_signature(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut wrong_version = signed.clone();
        wrong_version.version = 2;
        assert!(parse_release_signature(&wrong_version.to_bytes()).is_err());
        assert_eq!(
            parse_release_signature(&vec![b' '; MAX_RELEASE_SIGNATURE_BYTES + 1]),
            Err(SigningError::TooLarge("release signature"))
        );
        assert!(parse_release_signature(b"{}").is_err());
    }
}
