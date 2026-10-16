//! Publisher signatures for course releases and catalogs (ADR 0014).
//!
//! Pure logic: keys, canonical statements, signing, verification, rotation
//! chains, revocations, and catalog documents. No storage, OS, or IPC.
//! Callers supply randomness and untrusted bytes.

mod catalog;
mod keys;
mod release;
mod statements;

pub use catalog::{
    CATALOG_FORMAT, Catalog, CatalogEntry, CatalogStatement, MAX_CATALOG_BYTES,
    MAX_CATALOG_ENTRIES, VerifiedCatalog, parse_catalog, sign_catalog, verify_catalog,
};
pub use keys::{PublicKey, SecretKey, fingerprint};
pub use release::{
    MAX_RELEASE_SIGNATURE_BYTES, RELEASE_SIGNATURE_FILE, RELEASE_SIGNATURE_FORMAT,
    ReleaseSignature, ReleaseStatement, VerifiedRelease, parse_release_signature, sign_release,
    verify_release,
};
pub use statements::{
    MAX_ROTATIONS, RevocationRecord, RevocationStatement, RotationStatement, Signed, revoke,
    revoked, revoked_by_records, rotate, verify_chain,
};

use thiserror::Error;

/// Why a key, signature, statement, or document was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SigningError {
    /// A key is not `ed25519-` followed by 64 lowercase hex characters, or
    /// is not a valid curve point.
    #[error("the publisher key is not valid")]
    InvalidKey,
    /// The document is not JSON of the expected shape, format, or version.
    #[error("the {0} is not a valid Zeron document")]
    Malformed(&'static str),
    /// The document is larger than its bound.
    #[error("the {0} is too large")]
    TooLarge(&'static str),
    /// A signature does not verify.
    #[error("the {0} signature does not verify")]
    BadSignature(&'static str),
    /// The signed statement does not describe what it accompanies.
    #[error("the signature is for {0}")]
    Mismatch(&'static str),
    /// The rotation chain does not lead from the publisher's root key to the
    /// signing key.
    #[error("the publisher's key rotations do not lead to the signing key")]
    BrokenChain,
}
