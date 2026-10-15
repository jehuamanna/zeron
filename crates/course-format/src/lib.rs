//! Versioned course-manifest types and validation.
//!
//! This crate is deliberately independent of filesystems, databases, torrent
//! engines, and desktop frameworks. Callers provide the untrusted manifest
//! bytes; this crate parses and validates the logical contract.

mod manifest;
mod validation;

pub use manifest::{
    COURSE_SCHEMA_VERSION, CourseManifest, ExternalReference, Lesson, License, Module, Publisher,
    Resource, ResourceKind, ResourceRelation, ResourceRelationKind,
};
pub use validation::{ManifestError, ValidationIssue};

use schemars::Schema;

/// Parse untrusted JSON and enforce all version-one manifest invariants.
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
