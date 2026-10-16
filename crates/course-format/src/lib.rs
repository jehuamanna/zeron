//! Versioned course-manifest types and validation.
//!
//! This crate is deliberately independent of filesystems, databases, torrent
//! engines, and desktop frameworks. Callers provide the untrusted manifest
//! bytes; this crate parses and validates the logical contract.

mod manifest;
mod validation;

pub use manifest::{
    AccessModel, COURSE_SCHEMA_VERSION, CourseAccess, CourseManifest, ENCRYPTION_CHUNK_SIZE,
    ENCRYPTION_SCHEME, ENCRYPTION_TAG_SIZE, ExternalReference, Lesson, License, Module,
    PAID_COURSE_SCHEMA_VERSION, Publisher, Resource, ResourceEncryption, ResourceKind,
    ResourceRelation, ResourceRelationKind, sealed_byte_length,
};
pub use validation::{ManifestError, ValidationIssue};

use schemars::Schema;

/// Parse untrusted JSON and enforce all manifest invariants (versions 1 and 2).
///
/// # Errors
///
/// Returns [`ManifestError::Json`] when the bytes do not match the strict JSON
/// contract, or [`ManifestError::Validation`] when semantic invariants fail.
pub fn parse_and_validate(input: &[u8]) -> Result<CourseManifest, ManifestError> {
    let manifest: CourseManifest = serde_json::from_slice(input)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Return the generated JSON Schema for the current manifest type.
#[must_use]
pub fn manifest_schema() -> Schema {
    schemars::schema_for!(CourseManifest)
}
