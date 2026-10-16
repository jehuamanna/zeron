//! Signed catalogs: a publisher's list of releases, hosted anywhere.

use serde::{Deserialize, Serialize};

use crate::{
    PublicKey, SecretKey, SigningError,
    statements::{
        MAX_ROTATIONS, RevocationStatement, RotationStatement, Signed, canonical, revoked,
        verify_chain,
    },
};

/// The `format` value every catalog carries.
pub const CATALOG_FORMAT: &str = "zeron.catalog";
/// Largest catalog read.
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;
/// Most entries a catalog may list.
pub const MAX_CATALOG_ENTRIES: usize = 1_000;
const MAX_REVOCATIONS: usize = 64;
const MAX_TEXT_CHARS: usize = 2_000;
const MAX_MAGNET_BYTES: usize = 8 * 1024;
const VERSION: u32 = 1;
const DOMAIN: &[u8] = b"zeron.catalog.v1\n";

/// One release listed in a catalog. It points at the release; the release's
/// own signature is what learners trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEntry {
    /// The release's `courseId`.
    pub course_id: String,
    /// The release's version.
    pub course_version: String,
    /// Display title.
    pub title: String,
    /// Description; empty when there is none.
    pub description: String,
    /// Magnet link of the release torrent.
    pub magnet: String,
    /// v1 info hash, 40 lowercase hex characters.
    pub info_hash: String,
    /// Lowercase hex SHA-256 of the release's `course.json`.
    pub manifest_sha256: String,
}

/// What a publisher signs for a catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogStatement {
    /// The publisher's identity, the root key.
    pub publisher_id: String,
    /// The key that signed, the last of the rotation chain.
    pub publisher_key: String,
    /// Publisher or catalog name shown to learners.
    pub title: String,
    /// Unix seconds; a newer catalog of the same publisher replaces an older one.
    pub issued_at: u64,
    /// Listed releases.
    pub entries: Vec<CatalogEntry>,
    /// Keys of this publisher that must no longer be trusted.
    pub revocations: Vec<Signed<RevocationStatement>>,
}

/// A catalog document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    /// Always [`CATALOG_FORMAT`].
    pub format: String,
    /// Always 1.
    pub version: u32,
    /// The signed statement.
    pub statement: CatalogStatement,
    /// Signature by `statement.publisher_key`.
    pub signature: String,
    /// Rotations from the root key to the signing key.
    pub rotations: Vec<Signed<RotationStatement>>,
}

/// A catalog that verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCatalog {
    /// The publisher's identity.
    pub root: PublicKey,
    /// The key that signed.
    pub signing_key: PublicKey,
    /// When it was issued.
    pub issued_at: u64,
    /// The reason, when the signing key was revoked at the issue time by a
    /// revocation the catalog itself carries.
    pub revoked: Option<String>,
}

impl Catalog {
    /// The document as written to a file: pretty JSON with a final newline.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

/// Sign a catalog with the publisher's current key. `root` is the
/// publisher's identity and `rotations` the chain from it to `secret`'s key.
#[must_use]
pub fn sign_catalog(
    secret: &SecretKey,
    root: &PublicKey,
    title: &str,
    issued_at: u64,
    entries: Vec<CatalogEntry>,
    revocations: Vec<Signed<RevocationStatement>>,
    rotations: Vec<Signed<RotationStatement>>,
) -> Catalog {
    let statement = CatalogStatement {
        publisher_id: root.to_string(),
        publisher_key: secret.public_key().to_string(),
        title: title.to_owned(),
        issued_at,
        entries,
        revocations,
    };
    Catalog {
        format: CATALOG_FORMAT.to_owned(),
        version: VERSION,
        signature: secret.sign(&canonical(DOMAIN, &statement)),
        statement,
        rotations,
    }
}

/// Parse an untrusted catalog: bounded size and counts, strict JSON, and
/// well-formed entries.
///
/// # Errors
/// Returns [`SigningError::TooLarge`] or [`SigningError::Malformed`].
pub fn parse_catalog(bytes: &[u8]) -> Result<Catalog, SigningError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(SigningError::TooLarge("catalog"));
    }
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|_| SigningError::Malformed("catalog"))?;
    let statement = &catalog.statement;
    let well_formed = catalog.format == CATALOG_FORMAT
        && catalog.version == VERSION
        && catalog.rotations.len() <= MAX_ROTATIONS
        && statement.entries.len() <= MAX_CATALOG_ENTRIES
        && statement.revocations.len() <= MAX_REVOCATIONS
        && bounded(&statement.title)
        && statement.entries.iter().all(entry_is_well_formed);
    if well_formed {
        Ok(catalog)
    } else {
        Err(SigningError::Malformed("catalog"))
    }
}

/// Verify a catalog's signature and chain.
///
/// # Errors
/// Returns [`SigningError::BrokenChain`], [`SigningError::BadSignature`], or
/// [`SigningError::InvalidKey`].
pub fn verify_catalog(catalog: &Catalog) -> Result<VerifiedCatalog, SigningError> {
    let statement = &catalog.statement;
    let root: PublicKey = statement.publisher_id.parse()?;
    let signing_key: PublicKey = statement.publisher_key.parse()?;
    let chain = verify_chain(&root, &catalog.rotations)?;
    if chain.last() != Some(&signing_key) {
        return Err(SigningError::BrokenChain);
    }
    if !signing_key.verify(&canonical(DOMAIN, statement), &catalog.signature) {
        return Err(SigningError::BadSignature("catalog"));
    }
    Ok(VerifiedCatalog {
        root,
        signing_key,
        issued_at: statement.issued_at,
        revoked: revoked(
            &chain,
            &statement.revocations,
            &signing_key,
            statement.issued_at,
        )
        .map(|revocation| revocation.reason.clone()),
    })
}

fn bounded(text: &str) -> bool {
    text.chars().count() <= MAX_TEXT_CHARS && !text.chars().any(char::is_control)
}

fn entry_is_well_formed(entry: &CatalogEntry) -> bool {
    let id = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+')
            })
    };
    let hex = |value: &str, length: usize| {
        value.len() == length
            && value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    };
    id(&entry.course_id)
        && id(&entry.course_version)
        && bounded(&entry.title)
        && !entry.title.trim().is_empty()
        && entry.description.chars().count() <= MAX_TEXT_CHARS
        && hex(&entry.info_hash, 40)
        && hex(&entry.manifest_sha256, 64)
        && entry.magnet.len() <= MAX_MAGNET_BYTES
        && entry
            .magnet
            .to_ascii_lowercase()
            .starts_with(&format!("magnet:?xt=urn:btih:{}", entry.info_hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{revoke, rotate};

    fn key(seed: u8) -> SecretKey {
        SecretKey::from_bytes(&[seed; 32])
    }

    fn entry(version: &str) -> CatalogEntry {
        let info_hash = "a".repeat(40);
        CatalogEntry {
            course_id: "course_rust".to_owned(),
            course_version: version.to_owned(),
            title: "Rust Systems".to_owned(),
            description: String::new(),
            magnet: format!("magnet:?xt=urn:btih:{info_hash}&dn=rust"),
            info_hash,
            manifest_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn a_catalog_round_trips_and_verifies_after_a_rotation() {
        let (root, next) = (key(1), key(2));
        let catalog = sign_catalog(
            &next,
            &root.public_key(),
            "Ana's courses",
            100,
            vec![entry("1.0.0"), entry("1.1.0")],
            vec![revoke(&next, &root.public_key(), 50, "old laptop")],
            vec![rotate(&root, &next.public_key(), 40)],
        );
        let parsed = parse_catalog(&catalog.to_bytes()).unwrap();
        assert_eq!(parsed, catalog);
        let verified = verify_catalog(&parsed).unwrap();
        assert_eq!(verified.root, root.public_key());
        assert_eq!(verified.signing_key, next.public_key());
        assert_eq!(verified.revoked, None);
    }

    #[test]
    fn tampered_or_foreign_catalogs_are_refused() {
        let root = key(1);
        let catalog = sign_catalog(
            &root,
            &root.public_key(),
            "Ana",
            1,
            vec![entry("1.0.0")],
            Vec::new(),
            Vec::new(),
        );
        let mut tampered = catalog.clone();
        tampered.statement.entries[0]
            .magnet
            .push_str("&tr=udp://evil");
        assert_eq!(
            verify_catalog(&tampered),
            Err(SigningError::BadSignature("catalog"))
        );
        let mut claimed = sign_catalog(
            &key(9),
            &root.public_key(),
            "Ana",
            1,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        claimed.statement.publisher_key = key(9).public_key().to_string();
        assert_eq!(verify_catalog(&claimed), Err(SigningError::BrokenChain));
        let revoked_self = sign_catalog(
            &root,
            &root.public_key(),
            "Ana",
            10,
            Vec::new(),
            vec![revoke(&root, &root.public_key(), 5, "leaked")],
            Vec::new(),
        );
        assert_eq!(
            verify_catalog(&revoked_self).unwrap().revoked.as_deref(),
            Some("leaked")
        );
    }

    #[test]
    fn malformed_entries_and_oversized_catalogs_are_refused() {
        let root = key(1);
        let sign = |entries| {
            sign_catalog(
                &root,
                &root.public_key(),
                "Ana",
                1,
                entries,
                Vec::new(),
                Vec::new(),
            )
        };
        let mut wrong_magnet = entry("1.0.0");
        wrong_magnet.magnet = "https://example.com/course.torrent".to_owned();
        let mut wrong_hash = entry("1.0.0");
        wrong_hash.info_hash = "A".repeat(40);
        let mut control = entry("1.0.0");
        control.title = "Rust\u{0007}".to_owned();
        let mut path_like = entry("1.0.0");
        path_like.course_id = "../course".to_owned();
        for bad in [wrong_magnet, wrong_hash, control, path_like] {
            assert_eq!(
                parse_catalog(&sign(vec![bad]).to_bytes()),
                Err(SigningError::Malformed("catalog"))
            );
        }
        let too_many = vec![entry("1.0.0"); MAX_CATALOG_ENTRIES + 1];
        assert!(parse_catalog(&sign(too_many).to_bytes()).is_err());
        assert_eq!(
            parse_catalog(&vec![b' '; MAX_CATALOG_BYTES + 1]),
            Err(SigningError::TooLarge("catalog"))
        );
        assert!(parse_catalog(b"[]").is_err());
    }
}
