use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durable publishing stages exposed to applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PublishStage {
    /// Editable source state.
    Draft,
    /// Frozen snapshot is being checked.
    Validating,
    /// Files and manifest are being staged.
    Packaging,
    /// Torrent metainfo is being created.
    GeneratingTorrent,
    /// Artifacts are being independently checked.
    Verifying,
    /// Verified artifacts await explicit activation.
    ReadyToPublish,
    /// Immutable local release exists.
    Published,
    /// Published release is actively served.
    Seeding,
    /// Attempt failed with a recorded diagnostic.
    Failed,
    /// Mutable attempt was cancelled.
    Cancelled,
    /// A newer release is designated current.
    Superseded,
    /// Release is retained but hidden from active management.
    Archived,
}

/// Explicit publish workflow reconstructed from durable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishingWorkflow {
    stage: PublishStage,
}

/// Invalid publishing transition.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("publishing cannot transition from {from:?} to {to:?}")]
pub struct PublishTransitionError {
    /// Current durable stage.
    pub from: PublishStage,
    /// Requested next stage.
    pub to: PublishStage,
}

impl Default for PublishingWorkflow {
    fn default() -> Self {
        Self {
            stage: PublishStage::Draft,
        }
    }
}

impl PublishingWorkflow {
    /// Restore a workflow from its durable stage.
    #[must_use]
    pub const fn restore(stage: PublishStage) -> Self {
        Self { stage }
    }

    /// Current durable stage.
    #[must_use]
    pub const fn stage(&self) -> PublishStage {
        self.stage
    }

    /// Apply one legal transition.
    ///
    /// The caller persists transition evidence with the resulting stage.
    ///
    /// # Errors
    /// Returns an error for skipped, reversed, or terminal transitions.
    pub fn transition(&mut self, to: PublishStage) -> Result<(), PublishTransitionError> {
        let from = self.stage;
        if legal(from, to) {
            self.stage = to;
            Ok(())
        } else {
            Err(PublishTransitionError { from, to })
        }
    }
}

const fn legal(from: PublishStage, to: PublishStage) -> bool {
    use PublishStage::{
        Archived, Cancelled, Draft, Failed, GeneratingTorrent, Packaging, Published,
        ReadyToPublish, Seeding, Superseded, Validating, Verifying,
    };
    matches!(
        (from, to),
        (Draft, Validating)
            | (Validating, Packaging)
            | (Packaging, GeneratingTorrent)
            | (GeneratingTorrent, Verifying)
            | (Verifying, ReadyToPublish)
            | (ReadyToPublish, Published)
            | (Published, Seeding | Superseded | Archived)
            | (Seeding, Published | Superseded | Archived)
            | (Superseded, Archived)
            | (
                Draft | Validating | Packaging | GeneratingTorrent | Verifying | ReadyToPublish,
                Cancelled | Failed
            )
            | (Failed | Cancelled, Draft)
    )
}
