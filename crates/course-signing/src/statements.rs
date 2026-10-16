//! Signed statements, key rotation chains, and revocations.

use serde::{Deserialize, Serialize};

use crate::{PublicKey, SecretKey, SigningError};

/// Most rotations a chain may hold.
pub const MAX_ROTATIONS: usize = 32;
/// Longest revocation reason, in characters.
const MAX_REASON_CHARS: usize = 200;

const ROTATION_DOMAIN: &[u8] = b"zeron.key-rotation.v1\n";
const REVOCATION_DOMAIN: &[u8] = b"zeron.key-revocation.v1\n";

/// A statement with the signature over its canonical form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Signed<T> {
    /// What was signed.
    pub statement: T,
    /// Ed25519 signature, 128 lowercase hex characters.
    pub signature: String,
}

/// A retiring key hands the identity to its successor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RotationStatement {
    /// The key being retired; it signs this statement.
    pub previous_key: String,
    /// The key that signs from now on.
    pub next_key: String,
    /// Unix seconds.
    pub rotated_at: u64,
}

/// A key that must no longer be trusted from a point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevocationStatement {
    /// The key no longer trusted.
    pub revoked_key: String,
    /// Signatures made at or after this Unix time are not trusted.
    pub revoked_at: u64,
    /// Short reason shown to learners.
    pub reason: String,
}

/// A revocation kept together with the publisher's rotation chain that
/// shows who may sign it. Releases signed with a stolen retired key need not
/// carry the rotations that follow it, so revocations are checked against
/// the chain that arrived with them, from a catalog the publisher signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevocationRecord {
    /// The publisher's identity (root key).
    pub publisher_id: String,
    /// Rotations from the root, as the catalog carried them.
    pub rotations: Vec<Signed<RotationStatement>>,
    /// The revocation.
    pub revocation: Signed<RevocationStatement>,
}

/// The reason, when `records` hold a valid revocation of `signing_key` for
/// a signature made at `signed_at` by `root`'s publisher. A record counts
/// only when its chain verifies from `root` and the revocation is signed by
/// the revoked key or a key after it in that chain.
#[must_use]
pub fn revoked_by_records(
    root: &PublicKey,
    records: &[RevocationRecord],
    signing_key: &PublicKey,
    signed_at: u64,
) -> Option<String> {
    records_revoking(root, records, signing_key, |revoked_at| {
        signed_at >= revoked_at
    })
}

/// Whether a rotation made by `key` at `rotated_at` came after `key` was
/// revoked. A key revoked from the moment it was retired still hands over
/// validly at that moment.
pub(crate) fn rotation_revoked_by_records(
    root: &PublicKey,
    records: &[RevocationRecord],
    key: &PublicKey,
    rotated_at: u64,
) -> bool {
    records_revoking(root, records, key, |revoked_at| rotated_at > revoked_at).is_some()
}

fn records_revoking(
    root: &PublicKey,
    records: &[RevocationRecord],
    key: &PublicKey,
    applies: impl Fn(u64) -> bool,
) -> Option<String> {
    records
        .iter()
        .filter(|record| record.publisher_id == root.to_string())
        .find_map(|record| {
            let keys = verify_chain(root, &record.rotations).ok()?;
            revoked_where(
                &keys,
                std::slice::from_ref(&record.revocation),
                key,
                &applies,
            )
            .map(|revocation| revocation.reason.clone())
        })
}

/// The canonical bytes a signature covers: a domain-separation line and
/// compact JSON with fields in declared order.
pub(crate) fn canonical<T: Serialize>(domain: &[u8], statement: &T) -> Vec<u8> {
    let mut bytes = domain.to_vec();
    // Serializing these plain structs cannot fail.
    bytes.extend(serde_json::to_vec(statement).unwrap_or_default());
    bytes
}

/// Retire `previous`, handing the identity to `next`.
#[must_use]
pub fn rotate(
    previous: &SecretKey,
    next: &PublicKey,
    rotated_at: u64,
) -> Signed<RotationStatement> {
    let statement = RotationStatement {
        previous_key: previous.public_key().to_string(),
        next_key: next.to_string(),
        rotated_at,
    };
    Signed {
        signature: previous.sign(&canonical(ROTATION_DOMAIN, &statement)),
        statement,
    }
}

/// Revoke `revoked_key` from `revoked_at`, signed by that key or a later key
/// of the same chain. The reason is bounded to 200 characters.
#[must_use]
pub fn revoke(
    signer: &SecretKey,
    revoked_key: &PublicKey,
    revoked_at: u64,
    reason: &str,
) -> Signed<RevocationStatement> {
    let statement = RevocationStatement {
        revoked_key: revoked_key.to_string(),
        revoked_at,
        reason: reason.chars().take(MAX_REASON_CHARS).collect(),
    };
    Signed {
        signature: signer.sign(&canonical(REVOCATION_DOMAIN, &statement)),
        statement,
    }
}

/// Follow a rotation chain from the root key and return every key in it,
/// root first; the last is the current key. Each rotation must be signed by
/// the key it retires, start from the previous rotation's next key, and not
/// go back in time.
///
/// # Errors
/// Returns [`SigningError::BrokenChain`] or [`SigningError::InvalidKey`].
pub fn verify_chain(
    root: &PublicKey,
    rotations: &[Signed<RotationStatement>],
) -> Result<Vec<PublicKey>, SigningError> {
    if rotations.len() > MAX_ROTATIONS {
        return Err(SigningError::BrokenChain);
    }
    let mut keys = vec![*root];
    let mut last_time = 0;
    for rotation in rotations {
        let previous: PublicKey = rotation.statement.previous_key.parse()?;
        let next: PublicKey = rotation.statement.next_key.parse()?;
        let current = keys[keys.len() - 1];
        if previous != current
            || keys.contains(&next)
            || rotation.statement.rotated_at < last_time
            || !previous.verify(
                &canonical(ROTATION_DOMAIN, &rotation.statement),
                &rotation.signature,
            )
        {
            return Err(SigningError::BrokenChain);
        }
        last_time = rotation.statement.rotated_at;
        keys.push(next);
    }
    Ok(keys)
}

/// The revocation that applies to a signature made by `signing_key` at
/// `signed_at`, if any. Only revocations signed by the revoked key itself or
/// by a key after it in `chain` count; others are ignored.
#[must_use]
pub fn revoked<'a>(
    chain: &[PublicKey],
    revocations: &'a [Signed<RevocationStatement>],
    signing_key: &PublicKey,
    signed_at: u64,
) -> Option<&'a RevocationStatement> {
    revoked_where(chain, revocations, signing_key, |revoked_at| {
        signed_at >= revoked_at
    })
}

fn revoked_where<'a>(
    chain: &[PublicKey],
    revocations: &'a [Signed<RevocationStatement>],
    signing_key: &PublicKey,
    applies: impl Fn(u64) -> bool,
) -> Option<&'a RevocationStatement> {
    let position = chain.iter().position(|key| key == signing_key)?;
    revocations
        .iter()
        .filter(|revocation| {
            revocation.statement.revoked_key == signing_key.to_string()
                && applies(revocation.statement.revoked_at)
        })
        .find(|revocation| {
            let message = canonical(REVOCATION_DOMAIN, &revocation.statement);
            chain[position..]
                .iter()
                .any(|signer| signer.verify(&message, &revocation.signature))
        })
        .map(|revocation| &revocation.statement)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn a_chain_leads_from_the_root_through_each_rotation() {
        let (root, second, third) = (key(1), key(2), key(3));
        let rotations = vec![
            rotate(&root, &second.public_key(), 10),
            rotate(&second, &third.public_key(), 20),
        ];
        assert_eq!(
            verify_chain(&root.public_key(), &rotations).unwrap(),
            [root.public_key(), second.public_key(), third.public_key()]
        );
        assert_eq!(
            verify_chain(&root.public_key(), &[]).unwrap(),
            [root.public_key()]
        );
    }

    #[test]
    fn broken_chains_are_refused() {
        let (root, second, third, stranger) = (key(1), key(2), key(3), key(9));
        let good = rotate(&root, &second.public_key(), 10);
        let skip = rotate(&second, &third.public_key(), 20);
        let root_key = root.public_key();
        assert_eq!(
            verify_chain(&root_key, std::slice::from_ref(&skip)),
            Err(SigningError::BrokenChain)
        );
        let forged = rotate(&stranger, &third.public_key(), 10);
        let mut tampered = good.clone();
        tampered.statement.next_key = third.public_key().to_string();
        assert_eq!(
            verify_chain(&root_key, &[tampered]),
            Err(SigningError::BrokenChain)
        );
        let mut impersonated = forged;
        impersonated.statement.previous_key = root_key.to_string();
        assert_eq!(
            verify_chain(&root_key, &[impersonated]),
            Err(SigningError::BrokenChain)
        );
        let backwards = rotate(&second, &third.public_key(), 5);
        assert_eq!(
            verify_chain(&root_key, &[good.clone(), backwards]),
            Err(SigningError::BrokenChain)
        );
        let cycle = rotate(&second, &root_key, 30);
        assert_eq!(
            verify_chain(&root_key, &[good, cycle]),
            Err(SigningError::BrokenChain)
        );
    }

    #[test]
    fn revocations_apply_from_their_time_and_only_from_the_chain() {
        let (root, second) = (key(1), key(2));
        let chain = [root.public_key(), second.public_key()];
        let by_successor = revoke(&second, &root.public_key(), 100, "laptop stolen");
        let by_self = revoke(&root, &root.public_key(), 50, "retired");
        let by_stranger = revoke(&key(9), &second.public_key(), 0, "fake");
        let all = [by_successor.clone(), by_stranger];
        let root_key = root.public_key();
        assert_eq!(
            revoked(&chain, &all, &root_key, 150).map(|r| r.reason.as_str()),
            Some("laptop stolen")
        );
        assert!(revoked(&chain, &all, &root_key, 99).is_none());
        assert!(revoked(&chain, &all, &second.public_key(), 500).is_none());
        assert!(revoked(&chain, std::slice::from_ref(&by_self), &root_key, 60).is_some());
        let mut forged = by_successor;
        forged.statement.revoked_at = 0;
        assert!(revoked(&chain, &[forged], &root_key, 10).is_none());
        assert_eq!(
            revoke(&root, &root_key, 1, &"x".repeat(500))
                .statement
                .reason
                .chars()
                .count(),
            200
        );
    }
}
