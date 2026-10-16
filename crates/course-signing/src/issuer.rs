//! Signed requests between Zeron and a license issuer (ADR 0017 decisions 5,
//! 6, and 8).
//!
//! - A publisher **delegates** to an issuer with a
//!   `zeron.issuer-delegation.v1` statement (see [`crate::sign_delegation`])
//!   and hands it over in a [`DelegationSubmission`] with its rotation chain.
//! - A publisher **registers a paid release** with a
//!   `zeron.release-registration.v1` statement carrying the release's content
//!   key sealed to the issuer, signed by its current key.
//! - A learner **requests a license** with a `zeron.license-request.v1`
//!   statement naming its encryption key, signed by its learner key, which
//!   proves both keys belong to the same learner.

use serde::{Deserialize, Serialize};

use crate::{
    DelegationStatement, PublicKey, SecretKey, SigningError,
    statements::{
        MAX_ROTATIONS, RevocationRecord, RotationStatement, Signed, canonical, revoked_by_records,
        rotation_revoked_by_records, verify_chain,
    },
};

const REGISTRATION_DOMAIN: &[u8] = b"zeron.release-registration.v1\n";
const REQUEST_DOMAIN: &[u8] = b"zeron.license-request.v1\n";
const DELEGATION_DOMAIN: &[u8] = b"zeron.issuer-delegation.v1\n";
const ENCRYPTION_PREFIX: &str = "x25519-";

/// A publisher hands its delegation to the issuer it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationSubmission {
    /// The publisher's identity, its root key.
    pub publisher_id: String,
    /// Rotations from the root to the delegating key.
    pub rotations: Vec<Signed<RotationStatement>>,
    /// The delegation.
    pub delegation: Signed<DelegationStatement>,
}

/// What a publisher signs to register a paid release with an issuer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseRegistration {
    /// The course.
    pub course_id: String,
    /// The course version.
    pub course_version: String,
    /// The release's content key sealed to the issuer's encryption key, as
    /// the `course-access` crate writes it.
    pub sealed_content_key: String,
    /// The issuer this registration is for.
    pub issuer_key: String,
    /// The key that signed: the last of the rotation chain.
    pub publisher_key: String,
    /// Unix seconds.
    pub registered_at: u64,
}

/// A signed release registration with the publisher's chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistrationSubmission {
    /// The publisher's identity, its root key.
    pub publisher_id: String,
    /// Rotations from the root to the signing key.
    pub rotations: Vec<Signed<RotationStatement>>,
    /// The registration.
    pub registration: Signed<ReleaseRegistration>,
}

/// What a learner signs to ask for a license.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseRequest {
    /// The course.
    pub course_id: String,
    /// The learner's signing key (`ed25519-…`); it signs this request.
    pub learner_key: String,
    /// The learner's encryption key (`x25519-…`) to seal content keys to.
    pub encryption_key: String,
    /// The issuer this request is for.
    pub issuer_key: String,
    /// Unix seconds; issuers refuse requests far from their clock.
    pub requested_at: u64,
}

/// Sign a release registration with the publisher's current key.
#[must_use]
pub fn sign_release_registration(
    publisher: &SecretKey,
    publisher_id: &PublicKey,
    rotations: Vec<Signed<RotationStatement>>,
    registration: (&str, &str, &str),
    issuer_key: &PublicKey,
    registered_at: u64,
) -> RegistrationSubmission {
    let (course_id, course_version, sealed_content_key) = registration;
    let statement = ReleaseRegistration {
        course_id: course_id.to_owned(),
        course_version: course_version.to_owned(),
        sealed_content_key: sealed_content_key.to_owned(),
        issuer_key: issuer_key.to_string(),
        publisher_key: publisher.public_key().to_string(),
        registered_at,
    };
    RegistrationSubmission {
        publisher_id: publisher_id.to_string(),
        rotations,
        registration: Signed {
            signature: publisher.sign(&canonical(REGISTRATION_DOMAIN, &statement)),
            statement,
        },
    }
}

/// Sign a license request with the learner's signing key.
#[must_use]
pub fn sign_license_request(
    learner: &SecretKey,
    course_id: &str,
    encryption_key: &[u8; 32],
    issuer_key: &PublicKey,
    requested_at: u64,
) -> Signed<LicenseRequest> {
    let statement = LicenseRequest {
        course_id: course_id.to_owned(),
        learner_key: learner.public_key().to_string(),
        encryption_key: format!("{ENCRYPTION_PREFIX}{}", hex(encryption_key)),
        issuer_key: issuer_key.to_string(),
        requested_at,
    };
    Signed {
        signature: learner.sign(&canonical(REQUEST_DOMAIN, &statement)),
        statement,
    }
}

/// A verified publisher chain: its root and every key in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedChain {
    /// The publisher's identity.
    pub root: PublicKey,
    /// Root first; the last is the current key.
    pub keys: Vec<PublicKey>,
}

fn chain(
    publisher_id: &str,
    rotations: &[Signed<RotationStatement>],
    revocations: &[RevocationRecord],
) -> Result<VerifiedChain, SigningError> {
    if rotations.len() > MAX_ROTATIONS {
        return Err(SigningError::BrokenChain);
    }
    let root: PublicKey = publisher_id.parse()?;
    let keys = verify_chain(&root, rotations)?;
    for (index, rotation) in rotations.iter().enumerate() {
        if rotation_revoked_by_records(
            &root,
            revocations,
            &keys[index],
            rotation.statement.rotated_at,
        ) {
            return Err(SigningError::BrokenChain);
        }
    }
    Ok(VerifiedChain { root, keys })
}

/// Verify a delegation submitted to the issuer `issuer_key`: signed by a key
/// of the publisher's chain that was not revoked at `validFrom`, naming this
/// issuer, with an `https://` address.
///
/// # Errors
/// Returns [`SigningError::Mismatch`] for another issuer,
/// [`SigningError::BadSignature`], [`SigningError::BrokenChain`], or
/// [`SigningError::InvalidKey`].
pub fn verify_delegation_submission(
    submission: &DelegationSubmission,
    issuer_key: &PublicKey,
    revocations: &[RevocationRecord],
) -> Result<VerifiedChain, SigningError> {
    let verified = chain(&submission.publisher_id, &submission.rotations, revocations)?;
    let statement = &submission.delegation.statement;
    if statement.issuer_key != issuer_key.to_string() {
        return Err(SigningError::Mismatch("another issuer"));
    }
    if !statement.issuer_url.starts_with("https://") {
        return Err(SigningError::Malformed("issuer delegation"));
    }
    let delegator: PublicKey = statement.delegated_by.parse()?;
    let signed = verified.keys.contains(&delegator)
        && delegator.verify(
            &canonical(DELEGATION_DOMAIN, statement),
            &submission.delegation.signature,
        );
    if !signed
        || revoked_by_records(
            &verified.root,
            revocations,
            &delegator,
            statement.valid_from,
        )
        .is_some()
    {
        return Err(SigningError::BadSignature("issuer delegation"));
    }
    Ok(verified)
}

/// Verify a release registration for the issuer `issuer_key`: signed by the
/// publisher's current key, not revoked when it signed.
///
/// # Errors
/// Returns [`SigningError::Mismatch`] for another issuer,
/// [`SigningError::BadSignature`], [`SigningError::BrokenChain`], or
/// [`SigningError::InvalidKey`].
pub fn verify_release_registration(
    submission: &RegistrationSubmission,
    issuer_key: &PublicKey,
    revocations: &[RevocationRecord],
) -> Result<VerifiedChain, SigningError> {
    let verified = chain(&submission.publisher_id, &submission.rotations, revocations)?;
    let statement = &submission.registration.statement;
    if statement.issuer_key != issuer_key.to_string() {
        return Err(SigningError::Mismatch("another issuer"));
    }
    let signer: PublicKey = statement.publisher_key.parse()?;
    if verified.keys.last() != Some(&signer) {
        return Err(SigningError::BrokenChain);
    }
    if !signer.verify(
        &canonical(REGISTRATION_DOMAIN, statement),
        &submission.registration.signature,
    ) || revoked_by_records(
        &verified.root,
        revocations,
        &signer,
        statement.registered_at,
    )
    .is_some()
    {
        return Err(SigningError::BadSignature("release registration"));
    }
    Ok(verified)
}

/// A license request that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicenseRequest {
    /// The course.
    pub course_id: String,
    /// The learner's signing key.
    pub learner_key: PublicKey,
    /// The learner's X25519 encryption key.
    pub encryption_key: [u8; 32],
    /// When it was signed.
    pub requested_at: u64,
}

/// Verify a learner's license request for the issuer `issuer_key`.
///
/// # Errors
/// Returns [`SigningError::Mismatch`] for another issuer,
/// [`SigningError::Malformed`] for a bad encryption key,
/// [`SigningError::BadSignature`], or [`SigningError::InvalidKey`].
pub fn verify_license_request(
    request: &Signed<LicenseRequest>,
    issuer_key: &PublicKey,
) -> Result<VerifiedLicenseRequest, SigningError> {
    let statement = &request.statement;
    if statement.issuer_key != issuer_key.to_string() {
        return Err(SigningError::Mismatch("another issuer"));
    }
    let learner_key: PublicKey = statement.learner_key.parse()?;
    let encryption_key = statement
        .encryption_key
        .strip_prefix(ENCRYPTION_PREFIX)
        .and_then(unhex32)
        .ok_or(SigningError::Malformed("license request"))?;
    if !learner_key.verify(&canonical(REQUEST_DOMAIN, statement), &request.signature) {
        return Err(SigningError::BadSignature("license request"));
    }
    Ok(VerifiedLicenseRequest {
        course_id: statement.course_id.clone(),
        learner_key,
        encryption_key,
        requested_at: statement.requested_at,
    })
}

fn hex(bytes: &[u8]) -> String {
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
    use crate::{revoke, rotate, sign_delegation};

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn delegations_verify_only_for_their_issuer_from_the_chain() {
        let (root, next, issuer) = (key(1), key(2), key(3));
        let submission = DelegationSubmission {
            publisher_id: root.public_key().to_string(),
            rotations: vec![rotate(&root, &next.public_key(), 5)],
            delegation: sign_delegation(&next, &issuer.public_key(), "https://l.example", 10),
        };
        let verified =
            verify_delegation_submission(&submission, &issuer.public_key(), &[]).unwrap();
        assert_eq!(verified.root, root.public_key());
        assert_eq!(
            verify_delegation_submission(&submission, &key(4).public_key(), &[]),
            Err(SigningError::Mismatch("another issuer"))
        );
        let stranger = DelegationSubmission {
            delegation: sign_delegation(&key(9), &issuer.public_key(), "https://l.example", 10),
            ..submission.clone()
        };
        assert_eq!(
            verify_delegation_submission(&stranger, &issuer.public_key(), &[]),
            Err(SigningError::BadSignature("issuer delegation"))
        );
        let plain = DelegationSubmission {
            delegation: sign_delegation(&next, &issuer.public_key(), "http://l.example", 10),
            ..submission
        };
        assert!(verify_delegation_submission(&plain, &issuer.public_key(), &[]).is_err());
    }

    #[test]
    fn registrations_must_be_signed_by_the_current_unrevoked_key() {
        let (root, next, issuer) = (key(1), key(2), key(3));
        let rotations = vec![rotate(&root, &next.public_key(), 5)];
        let register = |signer: &SecretKey, at| {
            sign_release_registration(
                signer,
                &root.public_key(),
                rotations.clone(),
                ("course_a", "1.0.0", "ab"),
                &issuer.public_key(),
                at,
            )
        };
        let current = register(&next, 10);
        assert!(verify_release_registration(&current, &issuer.public_key(), &[]).is_ok());
        assert_eq!(
            verify_release_registration(&register(&root, 10), &issuer.public_key(), &[]),
            Err(SigningError::BrokenChain)
        );
        let mut tampered = current.clone();
        tampered.registration.statement.course_version = "2.0.0".to_owned();
        assert_eq!(
            verify_release_registration(&tampered, &issuer.public_key(), &[]),
            Err(SigningError::BadSignature("release registration"))
        );
        let stolen = [RevocationRecord {
            publisher_id: root.public_key().to_string(),
            rotations: rotations.clone(),
            revocation: revoke(&next, &next.public_key(), 8, "stolen"),
        }];
        assert_eq!(
            verify_release_registration(&current, &issuer.public_key(), &stolen),
            Err(SigningError::BadSignature("release registration"))
        );
    }

    #[test]
    fn license_requests_prove_the_learner_holds_both_keys() {
        let (learner, issuer) = (key(7), key(3));
        let encryption = [9_u8; 32];
        let request = sign_license_request(&learner, "c", &encryption, &issuer.public_key(), 100);
        let verified = verify_license_request(&request, &issuer.public_key()).unwrap();
        assert_eq!(verified.learner_key, learner.public_key());
        assert_eq!(verified.encryption_key, encryption);
        assert_eq!(verified.requested_at, 100);

        let mut swapped = request.clone();
        swapped.statement.encryption_key = format!("x25519-{}", "11".repeat(32));
        assert_eq!(
            verify_license_request(&swapped, &issuer.public_key()),
            Err(SigningError::BadSignature("license request"))
        );
        assert_eq!(
            verify_license_request(&request, &key(4).public_key()),
            Err(SigningError::Mismatch("another issuer"))
        );
        let mut garbled = request;
        garbled.statement.encryption_key = "x25519-zz".to_owned();
        assert_eq!(
            verify_license_request(&garbled, &issuer.public_key()),
            Err(SigningError::Malformed("license request"))
        );
    }
}
