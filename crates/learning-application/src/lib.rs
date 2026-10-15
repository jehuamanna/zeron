//! Application use cases shared by Zeron desktop composition roots.

mod library;
mod local_import;

pub use library::{CourseImportReceipt, CourseLibrary, LibraryError};
pub use local_import::{
    CancellationFlag, ImportError, ImportLimits, ImportedCourse, VerifiedResource,
    import_local_course,
};
