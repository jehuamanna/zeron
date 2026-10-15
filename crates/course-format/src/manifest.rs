use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The only manifest schema version understood by this crate.
pub const COURSE_SCHEMA_VERSION: u32 = 1;

/// The root `course.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseManifest {
    /// Wire-format version, currently `1`.
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
    /// Ordered modules.
    pub modules: Vec<Module>,
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
    /// Exact byte length in the published package.
    pub byte_length: u64,
    /// Lowercase hexadecimal SHA-256 digest.
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
