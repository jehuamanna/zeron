use std::path::Path;

use course_format::CourseManifest;
use learning_storage::{Storage, StorageError, StoredCourseSummary};
use thiserror::Error;

use crate::{CancellationFlag, ImportError, ImportLimits, import_local_course};

/// Result of a verified and durably persisted local course import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseImportReceipt {
    /// Stable logical course ID.
    pub course_id: String,
    /// Imported course version.
    pub course_version: String,
    /// Display title.
    pub title: String,
    /// Number of verified resources.
    pub verified_resource_count: usize,
    /// Sum of verified resource bytes.
    pub total_resource_bytes: u64,
}

/// Course-library startup and use-case failures.
#[derive(Debug, Error)]
pub enum LibraryError {
    /// Local package validation or verification failed.
    #[error(transparent)]
    Import(#[from] ImportError),
    /// Durable storage failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Ready local course-library application service.
///
/// This type composes verification and persistence while keeping both adapters
/// out of UI and Tauri command handlers.
pub struct CourseLibrary {
    storage: Storage,
    import_limits: ImportLimits,
}

impl CourseLibrary {
    /// Open, migrate, and capability-check a file-backed course library.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] until durable storage is ready.
    pub fn open(database_path: &Path, import_limits: ImportLimits) -> Result<Self, LibraryError> {
        Ok(Self {
            storage: Storage::open(database_path)?,
            import_limits,
        })
    }

    /// Construct a test/development library using isolated in-memory storage.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] if migrations or mandatory `SQLite`
    /// capabilities fail.
    pub fn open_in_memory(import_limits: ImportLimits) -> Result<Self, LibraryError> {
        Ok(Self {
            storage: Storage::open_in_memory()?,
            import_limits,
        })
    }

    /// Verify a local package completely, then persist its snapshot atomically.
    ///
    /// No course becomes visible when verification fails or is cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Import`] for package failures and
    /// [`LibraryError::Storage`] when durable persistence fails.
    pub fn import_local_folder(
        &mut self,
        root: &Path,
        cancellation: &CancellationFlag,
    ) -> Result<CourseImportReceipt, LibraryError> {
        let imported = import_local_course(root, self.import_limits, cancellation)?;
        let receipt = CourseImportReceipt {
            course_id: imported.manifest.course_id.clone(),
            course_version: imported.manifest.version.clone(),
            title: imported.manifest.title.clone(),
            verified_resource_count: imported.resources.len(),
            total_resource_bytes: imported.total_resource_bytes,
        };
        self.storage
            .save_imported_course(&imported.manifest, imported.total_resource_bytes)?;
        Ok(receipt)
    }

    /// List durably imported courses.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query or decoding failure.
    pub fn courses(&self) -> Result<Vec<StoredCourseSummary>, LibraryError> {
        self.storage.list_courses().map_err(LibraryError::from)
    }

    /// Search durably imported course metadata using FTS5 syntax.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] when the query is invalid or storage
    /// fails.
    pub fn search(&self, query: &str) -> Result<Vec<StoredCourseSummary>, LibraryError> {
        self.storage
            .search_courses(query)
            .map_err(LibraryError::from)
    }

    /// Load the validated manifest snapshot for a logical course.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError::Storage`] on query, decoding, or validation
    /// failure.
    pub fn manifest(&self, course_id: &str) -> Result<Option<CourseManifest>, LibraryError> {
        self.storage
            .course_manifest(course_id)
            .map_err(LibraryError::from)
    }
}
