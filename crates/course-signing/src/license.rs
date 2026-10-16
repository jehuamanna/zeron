//! Licenses for paid courses and issuer delegations (ADR 0017 decisions 4
//! and 5).
//!
//! A license is a statement `zeron.license.v1` naming a course, the learner
//! key it is for, and one sealed content key per course version, signed by
//! its issuer. The issuer is either a key of the publisher's own rotation
//! chain (a publisher licensing a buyer by hand) or a separate issuer key the
//! publisher delegated to with a `zeron.issuer-delegation.v1` statement. A
//! license file carries the publisher's chain and the delegation, so the
//! Library verifies it offline against the course's pinned publisher.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    PublicKey, SecretKey, SigningError,
    statements::{
        MAX_ROTATIONS, RevocationRecord, RotationStatement, Signed, canonical,
        delegate_revoked_by_records, revoked_by_records, rotation_revoked_by_records, verify_chain,
    },
};

/// The `format` value every license file carries.
pub const LICENSE_FORMAT: &str = "zeron.license";
/// Largest license file read.
pub const MAX_LICENSE_BYTES: usize = 128 * 1024;
/// Most course versions one license may unlock.
pub const MAX_LICENSE_KEYS: usize = 256;
/// Longest issuer address, in bytes.
const MAX_ISSUER_URL_BYTES: usize = 2048;
const VERSION: u32 = 1;
const LICENSE_DOMAIN: &[u8] = b"zeron.license.v1\n";
const DELEGATION_DOMAIN: &[u8] = b"zeron.issuer-delegation.v1\n";

/// One course version's content key, sealed to the learner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicensedKey {
    /// The course version this key opens.
    pub course_version: String,
    /// The content key sealed to the learner's encryption key, as the
    /// `course-access` crate writes it.
    pub wrapped_content_key: String,
}

/// What an issuer signs for a learner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseStatement {
    /// The course's `courseId`.
    pub course_id: String,
    /// The learner's signing key (`ed25519-…`), the identity the license is
    /// for.
    pub learner_key: String,
    /// Sealed content keys, one per course version.
    pub keys: Vec<LicensedKey>,
    /// The key that signed: a key of the publisher's chain, or the delegated
    /// issuer key.
    pub issuer_key: String,
    /// Unix seconds.
    pub issued_at: u64,
}

/// A publisher authorizes an issuer key to sign licenses for its courses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationStatement {
    /// The issuer's signing key.
    pub issuer_key: String,
    /// Where the issuer answers, `https://…`.
    pub issuer_url: String,
    /// Licenses issued before this Unix time are not covered.
    pub valid_from: u64,
    /// The publisher key that delegates: a key of its rotation chain.
    pub delegated_by: String,
}

/// The contents of a license file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseFile {
    /// Always [`LICENSE_FORMAT`].
    pub format: String,
    /// Always 1.
    pub version: u32,
    /// The publisher's identity, its root key.
    pub publisher_id: String,
    /// Rotations from the root key, as far as the signer or delegator needs.
    pub rotations: Vec<Signed<RotationStatement>>,
    /// Present when a delegated issuer signed the license.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<Signed<DelegationStatement>>,
    /// The license.
    pub license: Signed<LicenseStatement>,
}

impl LicenseFile {
    /// The file as saved: pretty JSON with a final newline.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

/// A license that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicense {
    /// The licensed course.
    pub course_id: String,
    /// Sealed content keys, one per course version.
    pub keys: Vec<LicensedKey>,
    /// The key that signed.
    pub issuer_key: PublicKey,
    /// Whether a delegated issuer signed, rather than the publisher.
    pub delegated: bool,
    /// When it was issued.
    pub issued_at: u64,
    /// The reason, when the issuer key was revoked at the issuing time.
    pub revoked: Option<String>,
}

/// Sign a license with `issuer`, for `learner_key`.
#[must_use]
pub fn sign_license(
    issuer: &SecretKey,
    course_id: &str,
    learner_key: &PublicKey,
    keys: Vec<LicensedKey>,
    issued_at: u64,
) -> Signed<LicenseStatement> {
    let statement = LicenseStatement {
        course_id: course_id.to_owned(),
        learner_key: learner_key.to_string(),
        keys,
        issuer_key: issuer.public_key().to_string(),
        issued_at,
    };
    Signed {
        signature: issuer.sign(&canonical(LICENSE_DOMAIN, &statement)),
        statement,
    }
}

/// Delegate license issuing to `issuer_key`, signed with a publisher key.
#[must_use]
pub fn sign_delegation(
    publisher: &SecretKey,
    issuer_key: &PublicKey,
    issuer_url: &str,
    valid_from: u64,
) -> Signed<DelegationStatement> {
    let statement = DelegationStatement {
        issuer_key: issuer_key.to_string(),
        issuer_url: issuer_url.to_owned(),
        valid_from,
        delegated_by: publisher.public_key().to_string(),
    };
    Signed {
        signature: publisher.sign(&canonical(DELEGATION_DOMAIN, &statement)),
        statement,
    }
}

/// Put a license, and the delegation behind it if any, into a file with the
/// publisher's chain.
#[must_use]
pub fn license_file(
    publisher_id: &PublicKey,
    rotations: Vec<Signed<RotationStatement>>,
    delegation: Option<Signed<DelegationStatement>>,
    license: Signed<LicenseStatement>,
) -> LicenseFile {
    LicenseFile {
        format: LICENSE_FORMAT.to_owned(),
        version: VERSION,
        publisher_id: publisher_id.to_string(),
        rotations,
        delegation,
        license,
    }
}

/// Parse an untrusted license file.
///
/// # Errors
/// Returns [`SigningError::TooLarge`] or [`SigningError::Malformed`].
pub fn parse_license(bytes: &[u8]) -> Result<LicenseFile, SigningError> {
    if bytes.len() > MAX_LICENSE_BYTES {
        return Err(SigningError::TooLarge("license"));
    }
    let file: LicenseFile =
        serde_json::from_slice(bytes).map_err(|_| SigningError::Malformed("license"))?;
    let keys = &file.license.statement.keys;
    let mut versions = HashSet::new();
    let well_formed = file.format == LICENSE_FORMAT
        && file.version == VERSION
        && file.rotations.len() <= MAX_ROTATIONS
        && !keys.is_empty()
        && keys.len() <= MAX_LICENSE_KEYS
        && keys
            .iter()
            .all(|key| versions.insert(key.course_version.as_str()))
        && file.delegation.as_ref().is_none_or(|delegation| {
            let url = &delegation.statement.issuer_url;
            url.starts_with("https://") && url.len() <= MAX_ISSUER_URL_BYTES
        });
    if !well_formed {
        return Err(SigningError::Malformed("license"));
    }
    Ok(file)
}

/// Verify a license for the learner `learner_key` against the publisher the
/// course is pinned to (`publisher_id`, its root key).
///
/// - The chain must lead from that root; no rotation in it may be made by a
///   key revoked by then.
/// - Without a delegation, the issuer must be a key of that chain.
/// - With one, the delegation must be signed by a key of the chain that was
///   not revoked at `validFrom`, name the issuer, and predate the license.
///
/// An issuer key revoked at the issuing time still verifies, with the reason
/// in [`VerifiedLicense::revoked`], so callers can say why they refuse it.
///
/// # Errors
/// Returns [`SigningError::Mismatch`] for another publisher, learner, or
/// issuer, [`SigningError::BrokenChain`], [`SigningError::BadSignature`], or
/// [`SigningError::InvalidKey`].
pub fn verify_license(
    file: &LicenseFile,
    publisher_id: &str,
    learner_key: &PublicKey,
    revocations: &[RevocationRecord],
) -> Result<VerifiedLicense, SigningError> {
    if file.publisher_id != publisher_id {
        return Err(SigningError::Mismatch("another publisher"));
    }
    let statement = &file.license.statement;
    if statement.learner_key != learner_key.to_string() {
        return Err(SigningError::Mismatch("another learner"));
    }
    let root: PublicKey = publisher_id.parse()?;
    let chain = verify_chain(&root, &file.rotations)?;
    // A revocation counts through the chain it arrived with or the
    // license's own chain.
    let records: Vec<RevocationRecord> = revocations
        .iter()
        .flat_map(|record| {
            [
                record.clone(),
                RevocationRecord {
                    rotations: file.rotations.clone(),
                    ..record.clone()
                },
            ]
        })
        .collect();
    for (index, rotation) in file.rotations.iter().enumerate() {
        if rotation_revoked_by_records(
            &root,
            &records,
            &chain[index],
            rotation.statement.rotated_at,
        ) {
            return Err(SigningError::BrokenChain);
        }
    }
    let issuer_key: PublicKey = statement.issuer_key.parse()?;
    let revoked = match &file.delegation {
        None => {
            if !chain.contains(&issuer_key) {
                return Err(SigningError::Mismatch(
                    "an issuer the publisher did not delegate",
                ));
            }
            revoked_by_records(&root, &records, &issuer_key, statement.issued_at)
        }
        Some(delegation) => {
            let delegated = &delegation.statement;
            let delegator: PublicKey = delegated.delegated_by.parse()?;
            if !chain.contains(&delegator)
                || !delegator.verify(
                    &canonical(DELEGATION_DOMAIN, delegated),
                    &delegation.signature,
                )
            {
                return Err(SigningError::BadSignature("issuer delegation"));
            }
            if revoked_by_records(&root, &records, &delegator, delegated.valid_from).is_some() {
                return Err(SigningError::BadSignature("issuer delegation"));
            }
            if delegated.issuer_key != statement.issuer_key {
                return Err(SigningError::Mismatch(
                    "an issuer the publisher did not delegate",
                ));
            }
            if statement.issued_at < delegated.valid_from {
                return Err(SigningError::Mismatch(
                    "a time before the issuer was delegated",
                ));
            }
            delegate_revoked_by_records(&root, &records, &issuer_key, statement.issued_at)
        }
    };
    if !issuer_key.verify(
        &canonical(LICENSE_DOMAIN, statement),
        &file.license.signature,
    ) {
        return Err(SigningError::BadSignature("license"));
    }
    Ok(VerifiedLicense {
        course_id: statement.course_id.clone(),
        keys: statement.keys.clone(),
        issuer_key,
        delegated: file.delegation.is_some(),
        issued_at: statement.issued_at,
        revoked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{revoke, rotate};

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    fn keys() -> Vec<LicensedKey> {
        vec![LicensedKey {
            course_version: "1.0.0".to_owned(),
            wrapped_content_key: "ab".to_owned(),
        }]
    }

    fn round_trip(file: &LicenseFile) -> LicenseFile {
        parse_license(&file.to_bytes()).unwrap()
    }

    #[test]
    fn a_publisher_signed_license_verifies_for_its_learner() {
        let (publisher, learner) = (key(1), key(7).public_key());
        let file = round_trip(&license_file(
            &publisher.public_key(),
            Vec::new(),
            None,
            sign_license(&publisher, "course_a", &learner, keys(), 100),
        ));
        let root = publisher.public_key().to_string();
        let verified = verify_license(&file, &root, &learner, &[]).unwrap();
        assert_eq!(verified.course_id, "course_a");
        assert_eq!(verified.keys, keys());
        assert!(!verified.delegated);
        assert!(verified.revoked.is_none());

        assert_eq!(
            verify_license(&file, &root, &key(8).public_key(), &[]),
            Err(SigningError::Mismatch("another learner"))
        );
        assert_eq!(
            verify_license(&file, &key(2).public_key().to_string(), &learner, &[]),
            Err(SigningError::Mismatch("another publisher"))
        );
        let mut tampered = file.clone();
        tampered.license.statement.course_id = "course_b".to_owned();
        assert_eq!(
            verify_license(&tampered, &root, &learner, &[]),
            Err(SigningError::BadSignature("license"))
        );
    }

    #[test]
    fn a_rotated_key_signs_through_its_chain_and_strangers_do_not() {
        let (root, second, stranger, learner) = (key(1), key(2), key(9), key(7).public_key());
        let rotations = vec![rotate(&root, &second.public_key(), 10)];
        let file = license_file(
            &root.public_key(),
            rotations.clone(),
            None,
            sign_license(&second, "c", &learner, keys(), 20),
        );
        let root_id = root.public_key().to_string();
        assert!(verify_license(&file, &root_id, &learner, &[]).is_ok());
        let forged = license_file(
            &root.public_key(),
            rotations,
            None,
            sign_license(&stranger, "c", &learner, keys(), 20),
        );
        assert_eq!(
            verify_license(&forged, &root_id, &learner, &[]),
            Err(SigningError::Mismatch(
                "an issuer the publisher did not delegate"
            ))
        );
    }

    #[test]
    fn a_delegated_issuer_signs_from_its_delegation_on() {
        let (publisher, issuer, learner) = (key(1), key(3), key(7).public_key());
        let delegation = sign_delegation(
            &publisher,
            &issuer.public_key(),
            "https://licenses.example",
            50,
        );
        let root_id = publisher.public_key().to_string();
        let file = round_trip(&license_file(
            &publisher.public_key(),
            Vec::new(),
            Some(delegation.clone()),
            sign_license(&issuer, "c", &learner, keys(), 60),
        ));
        let verified = verify_license(&file, &root_id, &learner, &[]).unwrap();
        assert!(verified.delegated);
        assert_eq!(verified.issuer_key, issuer.public_key());

        let early = license_file(
            &publisher.public_key(),
            Vec::new(),
            Some(delegation.clone()),
            sign_license(&issuer, "c", &learner, keys(), 40),
        );
        assert_eq!(
            verify_license(&early, &root_id, &learner, &[]),
            Err(SigningError::Mismatch(
                "a time before the issuer was delegated"
            ))
        );
        let other_issuer = license_file(
            &publisher.public_key(),
            Vec::new(),
            Some(delegation),
            sign_license(&key(4), "c", &learner, keys(), 60),
        );
        assert_eq!(
            verify_license(&other_issuer, &root_id, &learner, &[]),
            Err(SigningError::Mismatch(
                "an issuer the publisher did not delegate"
            ))
        );
        let self_delegated = license_file(
            &publisher.public_key(),
            Vec::new(),
            Some(sign_delegation(
                &issuer,
                &issuer.public_key(),
                "https://x.example",
                0,
            )),
            sign_license(&issuer, "c", &learner, keys(), 60),
        );
        assert_eq!(
            verify_license(&self_delegated, &root_id, &learner, &[]),
            Err(SigningError::BadSignature("issuer delegation"))
        );
    }

    #[test]
    fn revoked_issuers_are_reported_and_revoked_delegators_refused() {
        let (publisher, issuer, learner) = (key(1), key(3), key(7).public_key());
        let root = publisher.public_key();
        let record = |revocation| RevocationRecord {
            publisher_id: root.to_string(),
            rotations: Vec::new(),
            revocation,
        };
        let direct = license_file(
            &root,
            Vec::new(),
            None,
            sign_license(&publisher, "c", &learner, keys(), 100),
        );
        let stolen = [record(revoke(&publisher, &root, 90, "laptop stolen"))];
        assert_eq!(
            verify_license(&direct, &root.to_string(), &learner, &stolen)
                .unwrap()
                .revoked
                .as_deref(),
            Some("laptop stolen")
        );

        let delegated = license_file(
            &root,
            Vec::new(),
            Some(sign_delegation(
                &publisher,
                &issuer.public_key(),
                "https://l.example",
                10,
            )),
            sign_license(&issuer, "c", &learner, keys(), 100),
        );
        let issuer_gone = [record(revoke(
            &publisher,
            &issuer.public_key(),
            80,
            "issuer shut down",
        ))];
        assert_eq!(
            verify_license(&delegated, &root.to_string(), &learner, &issuer_gone)
                .unwrap()
                .revoked
                .as_deref(),
            Some("issuer shut down")
        );
        // An issuer cannot be revoked by someone outside the publisher's chain.
        let forged = [record(revoke(&key(9), &issuer.public_key(), 0, "fake"))];
        assert!(
            verify_license(&delegated, &root.to_string(), &learner, &forged)
                .unwrap()
                .revoked
                .is_none()
        );
        // A delegation signed after the delegating key was revoked is refused.
        let early_theft = [record(revoke(&publisher, &root, 5, "stolen"))];
        assert_eq!(
            verify_license(&delegated, &root.to_string(), &learner, &early_theft),
            Err(SigningError::BadSignature("issuer delegation"))
        );
    }

    #[test]
    fn malformed_files_are_refused() {
        let publisher = key(1);
        let learner = key(7).public_key();
        let good = license_file(
            &publisher.public_key(),
            Vec::new(),
            None,
            sign_license(&publisher, "c", &learner, keys(), 1),
        );
        assert!(parse_license(&good.to_bytes()).is_ok());
        let mutate = |change: &dyn Fn(&mut LicenseFile)| {
            let mut file = good.clone();
            change(&mut file);
            parse_license(&file.to_bytes())
        };
        assert!(mutate(&|file| file.format = "zeron.catalog".to_owned()).is_err());
        assert!(mutate(&|file| file.license.statement.keys.clear()).is_err());
        assert!(
            mutate(&|file| {
                let duplicate = file.license.statement.keys[0].clone();
                file.license.statement.keys.push(duplicate);
            })
            .is_err()
        );
        assert!(
            mutate(&|file| {
                file.delegation = Some(sign_delegation(
                    &publisher,
                    &learner,
                    "http://plain.example",
                    0,
                ));
            })
            .is_err()
        );
        assert_eq!(
            parse_license(&vec![b' '; MAX_LICENSE_BYTES + 1]),
            Err(SigningError::TooLarge("license"))
        );
        assert_eq!(
            parse_license(b"{}"),
            Err(SigningError::Malformed("license"))
        );
    }
}
