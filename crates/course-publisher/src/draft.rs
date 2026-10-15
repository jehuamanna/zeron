use std::{collections::HashSet, path::PathBuf};

use course_format::ResourceKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Editable course metadata before immutable package creation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftMetadata {
    /// Stable identity independent of title and release version.
    pub course_id: String,
    /// Publisher-selected semantic version for the next release.
    pub version: String,
    /// Display title.
    pub title: String,
    /// Optional course description.
    pub description: Option<String>,
    /// Publisher display name.
    pub publisher_name: String,
    /// BCP-47-style content language.
    pub language: String,
    /// Optional category.
    pub category: Option<String>,
    /// Discovery tags.
    pub tags: Vec<String>,
    /// Suggested prior knowledge.
    pub prerequisites: Vec<String>,
    /// Optional license name or SPDX identifier.
    pub license_name: Option<String>,
}

/// Mutable publisher draft, kept separate from every published release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftCourse {
    /// Stable draft identity, independent of course and release identity.
    pub draft_id: String,
    /// Optimistic-concurrency revision incremented by every mutation.
    pub revision: u64,
    /// Course-level fields.
    pub metadata: DraftMetadata,
    /// Ordered modules; vector order is authoritative while editing.
    pub modules: Vec<DraftModule>,
}

/// Editable module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftModule {
    /// Stable module identity.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Ordered lessons.
    pub lessons: Vec<DraftLesson>,
}

/// Editable lesson.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftLesson {
    /// Stable lesson identity.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Optional estimated duration.
    pub estimated_duration_seconds: Option<u64>,
    /// Ordered lesson resources.
    pub resources: Vec<DraftResource>,
}

/// A publisher source reference before packaging and hashing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftResource {
    /// Stable resource identity.
    pub id: String,
    /// Logical resource kind.
    pub kind: ResourceKind,
    /// Backend-owned source file reference. Never expose this over IPC.
    pub source_path: PathBuf,
    /// Requested normalized path inside the published package.
    pub package_path: String,
    /// Declared media type, confirmed during packaging.
    pub media_type: String,
    /// Optional duration from publisher input or a later probe.
    pub duration_seconds: Option<u64>,
    /// Optional resource language.
    pub language: Option<String>,
}

/// Safe, field-addressable problem found before packaging begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftValidationIssue {
    /// Stable machine-readable classification.
    pub code: &'static str,
    /// Draft field path suitable for focusing the editor.
    pub field: String,
    /// Human-readable remediation.
    pub message: String,
}

/// Rejected revision-safe edit.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditError {
    /// Another writer saved a newer revision.
    #[error("draft revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict {
        /// Revision supplied by the editor.
        expected: u64,
        /// Current durable revision.
        actual: u64,
    },
    /// The requested stable ID was not found.
    #[error("{entity} '{id}' was not found")]
    NotFound {
        /// Entity classification.
        entity: &'static str,
        /// Missing stable ID.
        id: String,
    },
    /// The target vector position is outside the collection.
    #[error("target position {position} is outside the {entity} collection")]
    InvalidPosition {
        /// Entity classification.
        entity: &'static str,
        /// Requested zero-based position.
        position: usize,
    },
    /// A new entity reused an ID already present in the draft.
    #[error("ID '{id}' is already used in this draft")]
    DuplicateId {
        /// Rejected ID.
        id: String,
    },
}

impl DraftCourse {
    /// Replace metadata when the caller owns the current revision.
    ///
    /// # Errors
    /// Returns [`EditError::RevisionConflict`] for a stale snapshot.
    pub fn replace_metadata(
        &mut self,
        expected_revision: u64,
        metadata: DraftMetadata,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        self.metadata = metadata;
        Ok(self.advance_revision())
    }

    /// Move a module without changing its stable ID.
    ///
    /// # Errors
    /// Rejects stale revisions, missing IDs, and out-of-range positions.
    pub fn move_module(
        &mut self,
        expected_revision: u64,
        module_id: &str,
        target_position: usize,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        move_by_id(
            &mut self.modules,
            module_id,
            target_position,
            "module",
            |item| &item.id,
        )?;
        Ok(self.advance_revision())
    }

    /// Move a lesson within its current module without changing stable IDs.
    ///
    /// # Errors
    /// Rejects stale revisions, missing IDs, and out-of-range positions.
    pub fn move_lesson(
        &mut self,
        expected_revision: u64,
        module_id: &str,
        lesson_id: &str,
        target_position: usize,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        let module = self
            .modules
            .iter_mut()
            .find(|module| module.id == module_id)
            .ok_or_else(|| EditError::NotFound {
                entity: "module",
                id: module_id.to_owned(),
            })?;
        move_by_id(
            &mut module.lessons,
            lesson_id,
            target_position,
            "lesson",
            |item| &item.id,
        )?;
        Ok(self.advance_revision())
    }

    /// Append a module with a caller-generated stable ID.
    ///
    /// # Errors
    /// Rejects stale revisions and IDs already used anywhere in the draft.
    pub fn add_module(
        &mut self,
        expected_revision: u64,
        module: DraftModule,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        for id in std::iter::once(&module.id).chain(module.lessons.iter().map(|lesson| &lesson.id))
        {
            self.require_unused(id)?;
        }
        self.modules.push(module);
        Ok(self.advance_revision())
    }

    /// Change a module title without changing its stable ID.
    ///
    /// # Errors
    /// Rejects stale revisions and missing IDs.
    pub fn rename_module(
        &mut self,
        expected_revision: u64,
        module_id: &str,
        title: String,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        self.module_mut(module_id)?.title = title;
        Ok(self.advance_revision())
    }

    /// Remove a module and every lesson and resource reference it contains.
    ///
    /// Source files are never touched; only the draft reference is removed.
    ///
    /// # Errors
    /// Rejects stale revisions and missing IDs.
    pub fn remove_module(
        &mut self,
        expected_revision: u64,
        module_id: &str,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        let index = self
            .modules
            .iter()
            .position(|module| module.id == module_id)
            .ok_or_else(|| not_found("module", module_id))?;
        self.modules.remove(index);
        Ok(self.advance_revision())
    }

    /// Append a lesson to a module with a caller-generated stable ID.
    ///
    /// # Errors
    /// Rejects stale revisions, missing modules, and IDs already in use.
    pub fn add_lesson(
        &mut self,
        expected_revision: u64,
        module_id: &str,
        lesson: DraftLesson,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        self.require_unused(&lesson.id)?;
        for resource in &lesson.resources {
            self.require_unused(&resource.id)?;
        }
        self.module_mut(module_id)?.lessons.push(lesson);
        Ok(self.advance_revision())
    }

    /// Change a lesson title without changing its stable ID.
    ///
    /// # Errors
    /// Rejects stale revisions and missing IDs.
    pub fn rename_lesson(
        &mut self,
        expected_revision: u64,
        lesson_id: &str,
        title: String,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        self.modules
            .iter_mut()
            .flat_map(|module| module.lessons.iter_mut())
            .find(|lesson| lesson.id == lesson_id)
            .ok_or_else(|| not_found("lesson", lesson_id))?
            .title = title;
        Ok(self.advance_revision())
    }

    /// Remove a lesson and its resource references from whichever module holds it.
    ///
    /// Source files are never touched; only the draft reference is removed.
    ///
    /// # Errors
    /// Rejects stale revisions and missing IDs.
    pub fn remove_lesson(
        &mut self,
        expected_revision: u64,
        lesson_id: &str,
    ) -> Result<u64, EditError> {
        self.require_revision(expected_revision)?;
        let (module, index) = self
            .modules
            .iter_mut()
            .find_map(|module| {
                let index = module
                    .lessons
                    .iter()
                    .position(|lesson| lesson.id == lesson_id)?;
                Some((module, index))
            })
            .ok_or_else(|| not_found("lesson", lesson_id))?;
        module.lessons.remove(index);
        Ok(self.advance_revision())
    }

    /// Validate structure and source references before packaging.
    #[must_use]
    pub fn validate(&self) -> Vec<DraftValidationIssue> {
        let mut issues = Vec::new();
        required(&mut issues, "metadata.courseId", &self.metadata.course_id);
        required(&mut issues, "metadata.version", &self.metadata.version);
        required(&mut issues, "metadata.title", &self.metadata.title);
        required(
            &mut issues,
            "metadata.publisherName",
            &self.metadata.publisher_name,
        );
        required(&mut issues, "metadata.language", &self.metadata.language);
        if self.modules.is_empty() {
            issue(
                &mut issues,
                "empty_course",
                "modules",
                "add at least one module",
            );
        }
        let mut ids = HashSet::new();
        let mut package_paths = HashSet::new();
        for (module_index, module) in self.modules.iter().enumerate() {
            unique_id(
                &mut issues,
                &mut ids,
                &module.id,
                format!("modules[{module_index}].id"),
            );
            required(
                &mut issues,
                &format!("modules[{module_index}].title"),
                &module.title,
            );
            if module.lessons.is_empty() {
                issue(
                    &mut issues,
                    "empty_module",
                    format!("modules[{module_index}].lessons"),
                    "add at least one lesson",
                );
            }
            for (lesson_index, lesson) in module.lessons.iter().enumerate() {
                let lesson_field = format!("modules[{module_index}].lessons[{lesson_index}]");
                unique_id(
                    &mut issues,
                    &mut ids,
                    &lesson.id,
                    format!("{lesson_field}.id"),
                );
                required(&mut issues, &format!("{lesson_field}.title"), &lesson.title);
                for (resource_index, resource) in lesson.resources.iter().enumerate() {
                    let field = format!("{lesson_field}.resources[{resource_index}]");
                    unique_id(&mut issues, &mut ids, &resource.id, format!("{field}.id"));
                    required(
                        &mut issues,
                        &format!("{field}.packagePath"),
                        &resource.package_path,
                    );
                    required(
                        &mut issues,
                        &format!("{field}.mediaType"),
                        &resource.media_type,
                    );
                    if !package_paths.insert(resource.package_path.to_lowercase()) {
                        issue(
                            &mut issues,
                            "duplicate_package_path",
                            format!("{field}.packagePath"),
                            "choose a unique package path",
                        );
                    }
                    match std::fs::metadata(&resource.source_path) {
                        Ok(metadata) if metadata.is_file() => {}
                        _ => issue(
                            &mut issues,
                            "missing_source",
                            format!("{field}.sourcePath"),
                            "select an existing regular file",
                        ),
                    }
                }
            }
        }
        issues
    }

    fn require_revision(&self, expected: u64) -> Result<(), EditError> {
        if self.revision == expected {
            Ok(())
        } else {
            Err(EditError::RevisionConflict {
                expected,
                actual: self.revision,
            })
        }
    }

    fn advance_revision(&mut self) -> u64 {
        self.revision = self.revision.saturating_add(1);
        self.revision
    }

    fn module_mut(&mut self, module_id: &str) -> Result<&mut DraftModule, EditError> {
        self.modules
            .iter_mut()
            .find(|module| module.id == module_id)
            .ok_or_else(|| not_found("module", module_id))
    }

    fn require_unused(&self, id: &str) -> Result<(), EditError> {
        let used = self.modules.iter().any(|module| {
            module.id == id
                || module.lessons.iter().any(|lesson| {
                    lesson.id == id || lesson.resources.iter().any(|resource| resource.id == id)
                })
        });
        if used {
            Err(EditError::DuplicateId { id: id.to_owned() })
        } else {
            Ok(())
        }
    }
}

fn not_found(entity: &'static str, id: &str) -> EditError {
    EditError::NotFound {
        entity,
        id: id.to_owned(),
    }
}

fn move_by_id<T>(
    items: &mut Vec<T>,
    id: &str,
    target: usize,
    entity: &'static str,
    get_id: impl Fn(&T) -> &str,
) -> Result<(), EditError> {
    if target >= items.len() {
        return Err(EditError::InvalidPosition {
            entity,
            position: target,
        });
    }
    let source = items
        .iter()
        .position(|item| get_id(item) == id)
        .ok_or_else(|| EditError::NotFound {
            entity,
            id: id.to_owned(),
        })?;
    let item = items.remove(source);
    items.insert(target, item);
    Ok(())
}

fn required(issues: &mut Vec<DraftValidationIssue>, field: &str, value: &str) {
    if value.trim().is_empty() {
        issue(issues, "required", field, "enter a value");
    }
}

fn unique_id(
    issues: &mut Vec<DraftValidationIssue>,
    ids: &mut HashSet<String>,
    id: &str,
    field: String,
) {
    required(issues, &field, id);
    if !id.is_empty() && !ids.insert(id.to_owned()) {
        issue(issues, "duplicate_id", field, "generate a unique stable ID");
    }
}

fn issue(
    issues: &mut Vec<DraftValidationIssue>,
    code: &'static str,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    issues.push(DraftValidationIssue {
        code,
        field: field.into(),
        message: message.into(),
    });
}
