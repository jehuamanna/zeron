use std::collections::{HashMap, HashSet};

use semver::Version;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use url::Url;

use crate::{
    COURSE_SCHEMA_VERSION, CourseManifest, ENCRYPTION_CHUNK_SIZE, ENCRYPTION_SCHEME,
    PAID_COURSE_SCHEMA_VERSION, Resource, ResourceKind, sealed_byte_length,
};

const MAX_ID_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 4_096;

/// A failure to parse or validate an untrusted manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// JSON syntax or shape does not match the version-one contract.
    #[error("invalid manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The document parsed but violated one or more semantic invariants.
    #[error("manifest validation failed with {} issue(s)", .0.len())]
    Validation(Vec<ValidationIssue>),
}

/// One semantic validation failure with a stable logical location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// JSON-style field location.
    pub path: String,
    /// Human-readable invariant violation.
    pub message: String,
}

impl CourseManifest {
    /// Validate all schema-version-one semantic invariants.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::Validation`] containing every semantic issue
    /// found in this validation pass.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let mut validator = Validator::default();
        validator.manifest_metadata(self);

        let mut all_ids = HashMap::<&str, String>::new();
        let mut resource_ids = HashMap::<&str, (&ResourceKind, String)>::new();
        let mut folded_paths = HashMap::<String, String>::new();

        check_positions(
            &mut validator,
            "modules",
            self.modules.iter().map(|m| m.position),
        );
        for (module_index, module) in self.modules.iter().enumerate() {
            let module_path = format!("modules[{module_index}]");
            validator.unique_id(&format!("{module_path}.id"), &module.id, &mut all_ids);
            validator.non_empty(&format!("{module_path}.title"), &module.title);
            check_positions(
                &mut validator,
                &format!("{module_path}.lessons"),
                module.lessons.iter().map(|lesson| lesson.position),
            );

            for (lesson_index, lesson) in module.lessons.iter().enumerate() {
                let lesson_path = format!("{module_path}.lessons[{lesson_index}]");
                validator.unique_id(&format!("{lesson_path}.id"), &lesson.id, &mut all_ids);
                validator.non_empty(&format!("{lesson_path}.title"), &lesson.title);
                validator.duration(
                    &format!("{lesson_path}.estimatedDurationSeconds"),
                    lesson.estimated_duration_seconds,
                );

                for (resource_index, resource) in lesson.resources.iter().enumerate() {
                    let resource_path = format!("{lesson_path}.resources[{resource_index}]");
                    validator.unique_id(&format!("{resource_path}.id"), &resource.id, &mut all_ids);
                    if resource_ids
                        .insert(&resource.id, (&resource.kind, resource_path.clone()))
                        .is_some()
                    {
                        // The cross-namespace uniqueness check above already reports this.
                    }
                    validator.package_path(
                        &format!("{resource_path}.path"),
                        &resource.path,
                        &mut folded_paths,
                    );
                    validator
                        .media_type(&format!("{resource_path}.mediaType"), &resource.media_type);
                    validator.sha256(&format!("{resource_path}.sha256"), &resource.sha256);
                    validator.encryption(&resource_path, resource);
                    validator.duration(
                        &format!("{resource_path}.durationSeconds"),
                        resource.duration_seconds,
                    );
                    if let Some(language) = &resource.language {
                        validator.language(&format!("{resource_path}.language"), language);
                    }
                }
            }
        }

        if let Some(thumbnail_id) = &self.thumbnail_resource_id {
            match resource_ids.get(thumbnail_id.as_str()) {
                Some((ResourceKind::Image, _)) => {}
                Some((_, _)) => {
                    validator.issue("thumbnailResourceId", "must reference an image resource");
                }
                None => validator.issue("thumbnailResourceId", "references an unknown resource ID"),
            }
        }

        for (module_index, module) in self.modules.iter().enumerate() {
            for (lesson_index, lesson) in module.lessons.iter().enumerate() {
                for (resource_index, resource) in lesson.resources.iter().enumerate() {
                    for (relation_index, relation) in resource.relations.iter().enumerate() {
                        if !resource_ids.contains_key(relation.resource_id.as_str()) {
                            validator.issue(
                                format!(
                                    "modules[{module_index}].lessons[{lesson_index}].resources[{resource_index}].relations[{relation_index}].resourceId"
                                ),
                                "references an unknown resource ID",
                            );
                        }
                    }
                }
            }
        }

        validator.finish()
    }
}

#[derive(Default)]
struct Validator {
    issues: Vec<ValidationIssue>,
}

impl Validator {
    fn issue(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            path: path.into(),
            message: message.into(),
        });
    }

    fn finish(self) -> Result<(), ManifestError> {
        if self.issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestError::Validation(self.issues))
        }
    }

    fn encryption(&mut self, resource_path: &str, resource: &Resource) {
        let Some(encryption) = &resource.encryption else {
            return;
        };
        if encryption.scheme != ENCRYPTION_SCHEME {
            self.issue(
                format!("{resource_path}.encryption.scheme"),
                format!("must be {ENCRYPTION_SCHEME}"),
            );
        }
        if encryption.chunk_size != ENCRYPTION_CHUNK_SIZE {
            self.issue(
                format!("{resource_path}.encryption.chunkSize"),
                format!("must be {ENCRYPTION_CHUNK_SIZE}"),
            );
        }
        if resource.byte_length != sealed_byte_length(encryption.plaintext_length) {
            self.issue(
                format!("{resource_path}.byteLength"),
                "must be the sealed length of encryption.plaintextLength",
            );
        }
    }

    fn manifest_metadata(&mut self, manifest: &CourseManifest) {
        let required = manifest.required_schema_version();
        if ![COURSE_SCHEMA_VERSION, PAID_COURSE_SCHEMA_VERSION].contains(&manifest.schema_version) {
            self.issue(
                "schemaVersion",
                format!(
                    "unsupported schema version {}; expected {COURSE_SCHEMA_VERSION} or {PAID_COURSE_SCHEMA_VERSION}",
                    manifest.schema_version
                ),
            );
        } else if manifest.schema_version != required {
            self.issue(
                "schemaVersion",
                format!("must be {required} for this course's content"),
            );
        }
        let encrypted = manifest
            .resources()
            .any(|resource| resource.encryption.is_some());
        if manifest.access.is_some() && !encrypted {
            self.issue(
                "access",
                "a paid course must have at least one encrypted resource",
            );
        }
        if encrypted && manifest.access.is_none() {
            self.issue("access", "required when a resource is encrypted");
        }
        self.id("courseId", &manifest.course_id);
        if Version::parse(&manifest.version).is_err() {
            self.issue("version", "must be a valid semantic version");
        }
        self.non_empty("title", &manifest.title);
        self.non_empty("publisher.name", &manifest.publisher.name);
        if let Some(id) = &manifest.publisher.publisher_id {
            self.id("publisher.publisherId", id);
        }
        self.language("language", &manifest.language);
        self.duration(
            "estimatedDurationSeconds",
            manifest.estimated_duration_seconds,
        );
        if let Some(license) = &manifest.license {
            self.non_empty("license.name", &license.name);
            if let Some(url) = &license.url {
                self.https_url("license.url", url);
            }
        }
        for (index, reference) in manifest.references.iter().enumerate() {
            self.non_empty(&format!("references[{index}].label"), &reference.label);
            self.https_url(&format!("references[{index}].url"), &reference.url);
        }
    }

    fn non_empty(&mut self, path: &str, value: &str) {
        if value.trim().is_empty() {
            self.issue(path, "must not be empty");
        }
    }

    fn id(&mut self, path: &str, value: &str) {
        let mut chars = value.chars();
        let valid_first = chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric());
        let valid_rest = chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'));
        if value.is_empty() || value.len() > MAX_ID_BYTES || !valid_first || !valid_rest {
            self.issue(path, "must be 1–128 ASCII letters, digits, `_`, or `-`, beginning with a letter or digit");
        }
    }

    fn unique_id<'a>(&mut self, path: &str, value: &'a str, ids: &mut HashMap<&'a str, String>) {
        self.id(path, value);
        if let Some(first_path) = ids.insert(value, path.to_owned()) {
            self.issue(
                path,
                format!("duplicates ID first declared at {first_path}"),
            );
        }
    }

    fn duration(&mut self, path: &str, value: Option<f64>) {
        if value.is_some_and(|duration| !duration.is_finite() || duration < 0.0) {
            self.issue(path, "must be a finite non-negative number");
        }
    }

    fn language(&mut self, path: &str, value: &str) {
        let valid = !value.is_empty()
            && value.len() <= 63
            && value.split('-').all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.chars().all(|ch| ch.is_ascii_alphanumeric())
            });
        if !valid {
            self.issue(path, "must be a conservative BCP-47-style language tag");
        }
    }

    fn https_url(&mut self, path: &str, value: &str) {
        match Url::parse(value) {
            Ok(url) if url.scheme() == "https" && url.host_str().is_some() => {}
            _ => self.issue(path, "must be an absolute HTTPS URL"),
        }
    }

    fn media_type(&mut self, path: &str, value: &str) {
        let valid = value.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_graphic()
                        && !matches!(
                            byte,
                            b'(' | b')'
                                | b'<'
                                | b'>'
                                | b'@'
                                | b','
                                | b';'
                                | b':'
                                | b'\\'
                                | b'"'
                                | b'['
                                | b']'
                                | b'?'
                                | b'='
                                | b'{'
                                | b'}'
                        )
                })
        });
        if !valid {
            self.issue(path, "must be a valid MIME type without parameters");
        }
    }

    fn sha256(&mut self, path: &str, value: &str) {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            self.issue(path, "must be exactly 64 lowercase hexadecimal characters");
        }
    }

    fn package_path(&mut self, path: &str, value: &str, folded: &mut HashMap<String, String>) {
        let normalized: String = value.nfc().collect();
        let segments: Vec<&str> = value.split('/').collect();
        let valid = !value.is_empty()
            && value.len() <= MAX_PATH_BYTES
            && normalized == value
            && !value.starts_with('/')
            && !value.contains('\\')
            && !value.chars().any(char::is_control)
            && segments.iter().all(|segment| {
                !segment.is_empty()
                    && *segment != "."
                    && *segment != ".."
                    && !segment.contains(':')
                    && !segment.ends_with([' ', '.'])
                    && !is_windows_reserved(segment)
            });

        if !valid {
            self.issue(
                path,
                "must be an NFC-normalized, portable, package-relative path with safe segments",
            );
            return;
        }
        if value.eq_ignore_ascii_case("course.json") {
            self.issue(path, "course.json is reserved for the package manifest");
        }

        let folded_path = value
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if let Some(first_path) = folded.insert(folded_path, path.to_owned()) {
            self.issue(
                path,
                format!("has a cross-platform case collision with {first_path}"),
            );
        }
    }
}

fn check_positions(validator: &mut Validator, path: &str, positions: impl Iterator<Item = u32>) {
    let positions: Vec<u32> = positions.collect();
    let unique: HashSet<u32> = positions.iter().copied().collect();
    let contiguous = positions
        .iter()
        .enumerate()
        .all(|(index, position)| usize::try_from(*position) == Ok(index));
    if unique.len() != positions.len() || !contiguous {
        validator.issue(
            path,
            "positions must be unique contiguous integers starting at zero and match array order",
        );
    }
}

fn is_windows_reserved(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}
