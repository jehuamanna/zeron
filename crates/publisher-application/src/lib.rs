//! Admin publisher use cases shared by Zeron composition roots.

mod sources;
mod staging;
mod torrents;

use std::path::Path;

use course_publisher::{
    DraftCourse, DraftLesson, DraftMetadata, DraftModule, DraftResource, DraftValidationIssue,
    EditError,
};
pub use course_publisher::{FindingSeverity, MediaFinding};
pub use course_publisher::{
    PackagingCancellation, PackagingError, PackagingLimits, StagingProgress,
};
use publisher_storage::PublisherStorage;
pub use publisher_storage::{DraftSummary, PublisherStorageError, StoredDraft};
pub use sources::{ResourceMediaFindings, SourceInfo, inspect_draft_media, source_info};
pub use staging::{StagingReceipt, discard_abandoned_staging, stage_draft};
use thiserror::Error;
pub use torrent_metainfo::{HashProgress, TorrentError};
pub use torrents::{DistributionOptions, TorrentReceipt, generate_torrent, verify_staged_torrent};

const MAX_LIST_ENTRIES: usize = 32;
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 10_000;

/// Publisher-supplied course metadata for one autosave.
///
/// The logical course ID is deliberately absent: it is assigned at draft
/// creation and never changes through editing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataEdit {
    /// Intended semantic version of the next release.
    pub version: String,
    /// Display title.
    pub title: String,
    /// Optional multi-line description.
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

/// One revision-checked change to the module and lesson tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructureEdit {
    /// Append a module with a fresh opaque ID.
    AddModule {
        /// Display title.
        title: String,
    },
    /// Change a module title.
    RenameModule {
        /// Stable module ID.
        module_id: String,
        /// New display title.
        title: String,
    },
    /// Remove a module, its lessons, and their resource references.
    RemoveModule {
        /// Stable module ID.
        module_id: String,
    },
    /// Move a module to a zero-based position.
    MoveModule {
        /// Stable module ID.
        module_id: String,
        /// Target position.
        position: usize,
    },
    /// Append a lesson with a fresh opaque ID.
    AddLesson {
        /// Parent module ID.
        module_id: String,
        /// Display title.
        title: String,
    },
    /// Change a lesson title.
    RenameLesson {
        /// Stable lesson ID.
        lesson_id: String,
        /// New display title.
        title: String,
    },
    /// Remove a lesson and its resource references.
    RemoveLesson {
        /// Stable lesson ID.
        lesson_id: String,
    },
    /// Move a lesson to a zero-based position within its module.
    MoveLesson {
        /// Parent module ID.
        module_id: String,
        /// Stable lesson ID.
        lesson_id: String,
        /// Target position.
        position: usize,
    },
    /// Remove a resource reference; the source file is untouched.
    RemoveResource {
        /// Stable resource ID.
        resource_id: String,
    },
    /// Move a resource to a zero-based position within its lesson.
    MoveResource {
        /// Parent lesson ID.
        lesson_id: String,
        /// Stable resource ID.
        resource_id: String,
        /// Target position.
        position: usize,
    },
}

/// Draft use-case failure.
#[derive(Debug, Error)]
pub enum DraftServiceError {
    /// No draft has the requested ID.
    #[error("draft '{draft_id}' was not found")]
    NotFound {
        /// Requested draft ID.
        draft_id: String,
    },
    /// The editor's snapshot is older than the durable draft.
    #[error("draft revision conflict: expected {expected}, current {actual}")]
    RevisionConflict {
        /// Revision the editor started from.
        expected: u64,
        /// Durable revision that must be reloaded.
        actual: u64,
    },
    /// An edit exceeded a bound or contained disallowed characters.
    #[error("{field} {message}")]
    InvalidInput {
        /// Draft field path suitable for focusing the editor.
        field: String,
        /// Remediation text.
        message: &'static str,
    },
    /// A module or lesson referenced by an edit does not exist.
    #[error("{entity} '{id}' was not found")]
    EntityNotFound {
        /// Entity classification.
        entity: &'static str,
        /// Missing stable ID.
        id: String,
    },
    /// A move targeted a position outside the collection.
    #[error("position {position} is outside the {entity} collection")]
    InvalidPosition {
        /// Entity classification.
        entity: &'static str,
        /// Requested zero-based position.
        position: usize,
    },
    /// A selected source cannot be used. The message never contains the path.
    #[error("the selected source {0}")]
    SourceRejected(&'static str),
    /// Package staging failed or was cancelled.
    #[error(transparent)]
    Packaging(PackagingError),
    /// Torrent generation or verification failed or was cancelled.
    #[error(transparent)]
    Torrent(TorrentError),
    /// A staging attempt is unknown, incomplete, or was modified.
    #[error("{0}")]
    InvalidAttempt(&'static str),
    /// The OS random source could not create an identifier.
    #[error("cannot generate an opaque identifier: {0}")]
    IdGeneration(#[source] getrandom::Error),
    /// Durable storage failed.
    #[error(transparent)]
    Storage(PublisherStorageError),
}

impl From<PublisherStorageError> for DraftServiceError {
    fn from(error: PublisherStorageError) -> Self {
        match error {
            PublisherStorageError::RevisionConflict { expected, actual } => {
                Self::RevisionConflict { expected, actual }
            }
            other => Self::Storage(other),
        }
    }
}

/// Draft creation, autosave, and validation use cases.
pub struct PublisherDrafts {
    storage: PublisherStorage,
}

impl PublisherDrafts {
    /// Open the file-backed publisher database.
    ///
    /// # Errors
    /// Returns an error when the database cannot be opened or migrated.
    pub fn open(path: &Path) -> Result<Self, DraftServiceError> {
        Ok(Self {
            storage: PublisherStorage::open(path)?,
        })
    }

    /// Open an isolated in-memory database for tests.
    ///
    /// # Errors
    /// Returns an error when the database cannot be migrated.
    pub fn open_in_memory() -> Result<Self, DraftServiceError> {
        Ok(Self {
            storage: PublisherStorage::open_in_memory()?,
        })
    }

    /// List path-free draft summaries, most recently saved first.
    ///
    /// # Errors
    /// Returns an error when storage cannot be queried.
    pub fn list(&self) -> Result<Vec<DraftSummary>, DraftServiceError> {
        Ok(self.storage.list_drafts()?)
    }

    /// Create an empty draft with fresh opaque draft and course IDs.
    ///
    /// # Errors
    /// Returns an error when randomness or storage fails.
    pub fn create(&mut self) -> Result<StoredDraft, DraftServiceError> {
        let draft = DraftCourse {
            draft_id: opaque_id("draft")?,
            revision: 0,
            metadata: DraftMetadata {
                course_id: opaque_id("course")?,
                version: "0.1.0".to_owned(),
                title: "Untitled course".to_owned(),
                description: None,
                publisher_name: "Local publisher".to_owned(),
                language: "en".to_owned(),
                category: None,
                tags: Vec::new(),
                prerequisites: Vec::new(),
                license_name: None,
            },
            modules: Vec::new(),
        };
        let summary = self.storage.create_draft(&draft)?;
        Ok(StoredDraft {
            draft,
            updated_at_unix_seconds: summary.updated_at_unix_seconds,
        })
    }

    /// Load one draft snapshot.
    ///
    /// # Errors
    /// Returns an error when storage cannot be queried.
    pub fn draft(&self, draft_id: &str) -> Result<Option<StoredDraft>, DraftServiceError> {
        Ok(self.storage.stored_draft(draft_id)?)
    }

    /// Autosave metadata when the editor still owns the durable revision.
    ///
    /// # Errors
    /// Reports stale revisions as [`DraftServiceError::RevisionConflict`] and
    /// rejects unbounded or control-character input before any write.
    pub fn update_metadata(
        &mut self,
        draft_id: &str,
        expected_revision: u64,
        edit: MetadataEdit,
    ) -> Result<StoredDraft, DraftServiceError> {
        let mut draft = self.require(draft_id)?;
        let metadata = normalize(edit, draft.metadata.course_id.clone())?;
        draft
            .replace_metadata(expected_revision, metadata)
            .map_err(edit_error)?;
        self.save(expected_revision, draft)
    }

    /// Apply one module or lesson change when the editor owns the durable revision.
    ///
    /// Removing structure only drops draft references; source files are untouched.
    ///
    /// # Errors
    /// Reports stale revisions as [`DraftServiceError::RevisionConflict`],
    /// missing targets, out-of-range moves, and unbounded titles.
    pub fn edit_structure(
        &mut self,
        draft_id: &str,
        expected_revision: u64,
        edit: StructureEdit,
    ) -> Result<StoredDraft, DraftServiceError> {
        let mut draft = self.require(draft_id)?;
        let applied = match edit {
            StructureEdit::AddModule { title } => draft.add_module(
                expected_revision,
                DraftModule {
                    id: opaque_id("module")?,
                    title: line("module.title", title, MAX_TITLE_CHARS)?,
                    lessons: Vec::new(),
                },
            ),
            StructureEdit::RenameModule { module_id, title } => draft.rename_module(
                expected_revision,
                &module_id,
                line("module.title", title, MAX_TITLE_CHARS)?,
            ),
            StructureEdit::RemoveModule { module_id } => {
                draft.remove_module(expected_revision, &module_id)
            }
            StructureEdit::MoveModule {
                module_id,
                position,
            } => draft.move_module(expected_revision, &module_id, position),
            StructureEdit::AddLesson { module_id, title } => draft.add_lesson(
                expected_revision,
                &module_id,
                DraftLesson {
                    id: opaque_id("lesson")?,
                    title: line("lesson.title", title, MAX_TITLE_CHARS)?,
                    estimated_duration_seconds: None,
                    resources: Vec::new(),
                },
            ),
            StructureEdit::RenameLesson { lesson_id, title } => draft.rename_lesson(
                expected_revision,
                &lesson_id,
                line("lesson.title", title, MAX_TITLE_CHARS)?,
            ),
            StructureEdit::RemoveLesson { lesson_id } => {
                draft.remove_lesson(expected_revision, &lesson_id)
            }
            StructureEdit::MoveLesson {
                module_id,
                lesson_id,
                position,
            } => draft.move_lesson(expected_revision, &module_id, &lesson_id, position),
            StructureEdit::RemoveResource { resource_id } => {
                draft.remove_resource(expected_revision, &resource_id)
            }
            StructureEdit::MoveResource {
                lesson_id,
                resource_id,
                position,
            } => draft.move_resource(expected_revision, &lesson_id, &resource_id, position),
        };
        applied.map_err(edit_error)?;
        self.save(expected_revision, draft)
    }

    /// Attach a backend-selected source file to a lesson as a new resource.
    ///
    /// The canonical path is recorded only in the draft snapshot. Bytes are
    /// neither read nor copied until packaging.
    ///
    /// # Errors
    /// Rejects missing or non-regular files, stale revisions, and missing lessons.
    pub fn attach_source(
        &mut self,
        draft_id: &str,
        expected_revision: u64,
        lesson_id: &str,
        source: &Path,
    ) -> Result<StoredDraft, DraftServiceError> {
        let mut draft = self.require(draft_id)?;
        let source = std::fs::canonicalize(source)
            .map_err(|_| DraftServiceError::SourceRejected("is unavailable"))?;
        if !std::fs::metadata(&source).is_ok_and(|metadata| metadata.is_file()) {
            return Err(DraftServiceError::SourceRejected("is not a regular file"));
        }
        let file_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (kind, media_type) = sources::classify(&source);
        let duration_seconds =
            course_publisher::inspect_media(&source, media_type).duration_seconds;
        let id = opaque_id("resource")?;
        let package_path = format!("resources/{id}/{}", sources::package_file_name(&file_name));
        draft
            .add_resource(
                expected_revision,
                lesson_id,
                DraftResource {
                    id,
                    kind,
                    source_path: source,
                    package_path,
                    media_type: media_type.to_owned(),
                    duration_seconds,
                    language: None,
                },
            )
            .map_err(edit_error)?;
        self.save(expected_revision, draft)
    }

    /// Validate the saved draft with the same policy used before packaging.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::NotFound`] or a storage failure.
    pub fn validate(&self, draft_id: &str) -> Result<Vec<DraftValidationIssue>, DraftServiceError> {
        Ok(self.require(draft_id)?.validate())
    }

    fn save(
        &mut self,
        expected_revision: u64,
        draft: DraftCourse,
    ) -> Result<StoredDraft, DraftServiceError> {
        let summary = self.storage.update_draft(expected_revision, &draft)?;
        Ok(StoredDraft {
            draft,
            updated_at_unix_seconds: summary.updated_at_unix_seconds,
        })
    }

    fn require(&self, draft_id: &str) -> Result<DraftCourse, DraftServiceError> {
        self.storage
            .draft(draft_id)?
            .ok_or_else(|| DraftServiceError::NotFound {
                draft_id: draft_id.to_owned(),
            })
    }
}

fn normalize(edit: MetadataEdit, course_id: String) -> Result<DraftMetadata, DraftServiceError> {
    Ok(DraftMetadata {
        course_id,
        version: line("metadata.version", edit.version, 64)?,
        title: line("metadata.title", edit.title, MAX_TITLE_CHARS)?,
        description: optional(
            edit.description
                .map(|value| multiline("metadata.description", value))
                .transpose()?,
        ),
        publisher_name: line("metadata.publisherName", edit.publisher_name, 200)?,
        language: line("metadata.language", edit.language, 63)?,
        category: optional(
            edit.category
                .map(|value| line("metadata.category", value, 100))
                .transpose()?,
        ),
        tags: list("metadata.tags", edit.tags, 64)?,
        prerequisites: list("metadata.prerequisites", edit.prerequisites, 200)?,
        license_name: optional(
            edit.license_name
                .map(|value| line("metadata.licenseName", value, 100))
                .transpose()?,
        ),
    })
}

fn line(field: &str, value: String, max_chars: usize) -> Result<String, DraftServiceError> {
    if value.chars().count() > max_chars {
        return Err(invalid(field, "is too long"));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(field, "must be a single line of text"));
    }
    Ok(value)
}

fn multiline(field: &str, value: String) -> Result<String, DraftServiceError> {
    if value.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(invalid(field, "is too long"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
    {
        return Err(invalid(field, "contains unsupported control characters"));
    }
    Ok(value)
}

fn optional(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn list(
    field: &str,
    values: Vec<String>,
    max_chars: usize,
) -> Result<Vec<String>, DraftServiceError> {
    if values.len() > MAX_LIST_ENTRIES {
        return Err(invalid(field, "has too many entries"));
    }
    let mut entries = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        let value = line(&format!("{field}[{index}]"), value, max_chars)?;
        let value = value.trim();
        if !value.is_empty() && !entries.iter().any(|entry| entry == value) {
            entries.push(value.to_owned());
        }
    }
    Ok(entries)
}

fn invalid(field: &str, message: &'static str) -> DraftServiceError {
    DraftServiceError::InvalidInput {
        field: field.to_owned(),
        message,
    }
}

fn edit_error(error: EditError) -> DraftServiceError {
    match error {
        EditError::RevisionConflict { expected, actual } => {
            DraftServiceError::RevisionConflict { expected, actual }
        }
        EditError::NotFound { entity, id } => DraftServiceError::EntityNotFound { entity, id },
        EditError::InvalidPosition { entity, position } => {
            DraftServiceError::InvalidPosition { entity, position }
        }
        EditError::DuplicateId { .. } => DraftServiceError::InvalidInput {
            field: "id".to_owned(),
            message: "collided with an existing ID; retry the edit",
        },
    }
}

pub(crate) fn opaque_id(prefix: &str) -> Result<String, DraftServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(DraftServiceError::IdGeneration)?;
    let mut id = String::with_capacity(prefix.len() + 1 + bytes.len() * 2);
    id.push_str(prefix);
    id.push('_');
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}
