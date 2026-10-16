use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Manifest schema version of courses without paid content. Writers emit it
/// for every free course, so older readers keep importing them.
pub const COURSE_SCHEMA_VERSION: u32 = 1;

/// Manifest schema version of courses with paid content (ADR 0017). It adds
/// the course `access` object and per-resource `encryption`.
pub const PAID_COURSE_SCHEMA_VERSION: u32 = 2;

/// `encryption.scheme` of sealed resources (ADR 0017 decision 2).
pub const ENCRYPTION_SCHEME: &str = "zeron.chunked-xchacha20poly1305.v1";

/// `encryption.chunkSize`: plaintext bytes per sealed chunk.
pub const ENCRYPTION_CHUNK_SIZE: u32 = 65_520;

/// Authentication tag bytes added to every sealed chunk.
pub const ENCRYPTION_TAG_SIZE: u32 = 16;

/// Package byte length of a resource sealed from `plaintext_length` bytes.
#[must_use]
pub fn sealed_byte_length(plaintext_length: u64) -> u64 {
    let chunks = plaintext_length
        .div_ceil(u64::from(ENCRYPTION_CHUNK_SIZE))
        .max(1);
    plaintext_length + chunks * u64::from(ENCRYPTION_TAG_SIZE)
}

/// The root `course.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseManifest {
    /// Wire-format version: `1` without paid content, `2` with it.
    pub schema_version: u32,
    /// Stable logical identity, independent of title, path, and release.
    pub course_id: String,
    /// Publisher-selected semantic version.
    pub version: String,
    /// Human-readable course title.
    pub title: String,
    /// Optional course description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Publisher attribution.
    pub publisher: Publisher,
    /// BCP-47-style language tag. Version one performs conservative syntax checks.
    pub language: String,
    /// Optional publisher-defined category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Search and discovery tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Optional license metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,
    /// Suggested prior knowledge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<String>,
    /// External HTTPS references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<ExternalReference>,
    /// Resource ID used as the course thumbnail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_resource_id: Option<String>,
    /// Publisher-provided total duration estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_duration_seconds: Option<f64>,
    /// How paid content is unlocked. Present exactly when a resource is
    /// encrypted; schema version 2 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<CourseAccess>,
    /// Ordered modules.
    pub modules: Vec<Module>,
}

impl CourseManifest {
    /// The schema version this manifest's content requires.
    #[must_use]
    pub fn required_schema_version(&self) -> u32 {
        let encrypted = self
            .resources()
            .any(|resource| resource.encryption.is_some());
        if self.access.is_some() || encrypted {
            PAID_COURSE_SCHEMA_VERSION
        } else {
            COURSE_SCHEMA_VERSION
        }
    }

    /// Every resource in course order.
    pub fn resources(&self) -> impl Iterator<Item = &Resource> {
        self.modules
            .iter()
            .flat_map(|module| &module.lessons)
            .flat_map(|lesson| &lesson.resources)
    }
}

/// How a course's paid content is unlocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseAccess {
    /// Access model.
    pub model: AccessModel,
}

/// Supported access models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccessModel {
    /// Some lessons are encrypted and unlock with a license; the rest are
    /// free previews.
    Paid,
}

/// How a resource's package bytes are sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceEncryption {
    /// Must be [`ENCRYPTION_SCHEME`].
    pub scheme: String,
    /// Plaintext bytes per chunk; must be [`ENCRYPTION_CHUNK_SIZE`].
    pub chunk_size: u32,
    /// Length of the decrypted resource. `byteLength` is the sealed length.
    pub plaintext_length: u64,
}

impl ResourceEncryption {
    /// The encryption record for a resource of `plaintext_length` bytes.
    #[must_use]
    pub fn chunked(plaintext_length: u64) -> Self {
        Self {
            scheme: ENCRYPTION_SCHEME.to_owned(),
            chunk_size: ENCRYPTION_CHUNK_SIZE,
            plaintext_length,
        }
    }
}

/// Publisher attribution embedded in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publisher {
    /// Display name.
    pub name: String,
    /// Optional stable publisher identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_id: Option<String>,
}

/// Human-readable license metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct License {
    /// License name or SPDX identifier.
    pub name: String,
    /// Optional canonical HTTPS reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A labelled external HTTPS link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalReference {
    /// Link label.
    pub label: String,
    /// HTTPS URL.
    pub url: String,
}

/// An ordered course module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Module {
    /// Stable module ID.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Zero-based position within the course.
    pub position: u32,
    /// Ordered lessons.
    pub lessons: Vec<Lesson>,
}

/// An ordered lesson.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lesson {
    /// Stable lesson ID.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Zero-based position within the module.
    pub position: u32,
    /// Publisher-provided duration estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_duration_seconds: Option<f64>,
    /// Resources associated with the lesson.
    pub resources: Vec<Resource>,
}

/// A package resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    /// Stable resource ID.
    pub id: String,
    /// Logical resource kind.
    pub kind: ResourceKind,
    /// Safe, normalized package-relative path using `/` separators.
    pub path: String,
    /// Declared MIME media type.
    pub media_type: String,
    /// Exact byte length in the published package (sealed, if encrypted).
    pub byte_length: u64,
    /// Lowercase hexadecimal SHA-256 digest of the package bytes (sealed,
    /// if encrypted).
    pub sha256: String,
    /// Media duration when meaningful.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    /// Language tag for language-specific resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Typed relationships to other resources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<ResourceRelation>,
    /// Present when the package bytes are sealed; schema version 2 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<ResourceEncryption>,
}

impl Resource {
    /// Length of the bytes a reader sees: the plaintext length when sealed.
    #[must_use]
    pub fn content_length(&self) -> u64 {
        self.encryption
            .as_ref()
            .map_or(self.byte_length, |encryption| encryption.plaintext_length)
    }
}

/// Supported resource kinds in schema version one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Video media.
    Video,
    /// Audio media.
    Audio,
    /// PDF document.
    Pdf,
    /// Markdown document.
    Markdown,
    /// Plain text document.
    Text,
    /// Subtitle track.
    Subtitle,
    /// Navigable transcript.
    Transcript,
    /// Image asset.
    Image,
    /// Source-code archive.
    SourceArchive,
    /// A valid but otherwise unclassified resource.
    Other,
}

/// A typed relationship between two resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceRelation {
    /// Relationship meaning.
    pub kind: ResourceRelationKind,
    /// Referenced stable resource ID.
    pub resource_id: String,
}

/// Supported resource relationships in schema version one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRelationKind {
    /// A transcript for media.
    Transcript,
    /// A subtitle track for media.
    Subtitle,
    /// A thumbnail or poster image.
    Thumbnail,
    /// Supplemental material.
    Supplement,
}
