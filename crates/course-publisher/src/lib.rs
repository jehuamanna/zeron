//! Draft editing and publishing policy for Zeron course releases.

mod draft;
mod packaging;
mod workflow;

pub use draft::{
    DraftCourse, DraftLesson, DraftMetadata, DraftModule, DraftResource, DraftValidationIssue,
    EditError,
};
pub use packaging::{
    MANIFEST_FILE_NAME, PackagingCancellation, PackagingError, PackagingLimits, StagedFile,
    StagedPackage, StagingProgress, canonical_manifest_bytes, manifest_for, stage_package,
};
pub use workflow::{PublishStage, PublishTransitionError, PublishingWorkflow};
