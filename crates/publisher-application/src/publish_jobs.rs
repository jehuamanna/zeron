use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use course_publisher::{
    DraftCourse, PackagingCancellation, PackagingError, PackagingLimits, PublishStage,
    PublishingWorkflow,
};
use torrent_metainfo::TorrentError;

use publisher_storage::ReleaseRecord;

use crate::{
    DistributionOptions, DraftServiceError, PublisherDrafts, StagingReceipt, TorrentPhase,
    TorrentReceipt, discard_all_attempts, discard_attempt, generate_torrent, opaque_id,
    stage_draft,
};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const RETAINED_FINISHED_JOBS: usize = 16;

/// Byte progress within the current stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobProgress {
    /// Bytes processed in this stage.
    pub completed_bytes: u64,
    /// Bytes this stage will process.
    pub total_bytes: u64,
}

/// Verified artifacts awaiting explicit activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRelease {
    /// Staged package facts.
    pub staging: StagingReceipt,
    /// Verified torrent facts.
    pub torrent: TorrentReceipt,
}

/// Path-free failure classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFailure {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable description.
    pub message: String,
}

/// Point-in-time state of one release preparation. The snapshot is the
/// source of truth; notifications are hints carrying the same value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishJobSnapshot {
    /// Opaque job ID.
    pub job_id: String,
    /// Draft being prepared.
    pub draft_id: String,
    /// Draft revision frozen into this job.
    pub draft_revision: u64,
    /// Monotonic version; larger values supersede smaller ones.
    pub sequence: u64,
    /// Current workflow stage.
    pub stage: PublishStage,
    /// Progress within the stage when it processes bytes.
    pub progress: Option<JobProgress>,
    /// Artifacts once the job reaches `ReadyToPublish`.
    pub prepared: Option<PreparedRelease>,
    /// Failure details when the job reaches `Failed`.
    pub failure: Option<JobFailure>,
    /// Release created from this job once it reaches `Published`.
    pub release_id: Option<String>,
}

impl PublishJobSnapshot {
    /// Whether the job can no longer change.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(
            self.stage,
            PublishStage::ReadyToPublish
                | PublishStage::Published
                | PublishStage::Failed
                | PublishStage::Cancelled
        )
    }
}

struct Entry {
    snapshot: PublishJobSnapshot,
    cancellation: PackagingCancellation,
    finished_order: Option<u64>,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, Entry>,
    active: Option<String>,
    finished_counter: u64,
}

/// In-process registry and runner for release preparation jobs.
///
/// At most one job runs at a time to bound disk and CPU use.
pub struct PublishJobs {
    staging_root: PathBuf,
    registry: Mutex<Registry>,
}

impl PublishJobs {
    /// Create a registry that stages under `staging_root`.
    #[must_use]
    pub fn new(staging_root: &Path) -> Self {
        Self {
            staging_root: staging_root.to_path_buf(),
            registry: Mutex::new(Registry::default()),
        }
    }

    /// Remove every attempt left by a previous process. Preparations are
    /// session-scoped, so nothing can activate them after a restart. Call once
    /// at startup, before any job begins.
    ///
    /// # Errors
    /// Returns an I/O failure from the staging root.
    pub fn recover(&self) -> Result<usize, DraftServiceError> {
        discard_all_attempts(&self.staging_root)
    }

    /// Activate a `ReadyToPublish` job into an immutable release.
    ///
    /// # Errors
    /// Rejects unknown or unready jobs, reused versions, and activation failures.
    pub fn activate(
        &self,
        job_id: &str,
        drafts: &mut PublisherDrafts,
        releases_root: &Path,
    ) -> Result<(ReleaseRecord, PublishJobSnapshot), DraftServiceError> {
        let snapshot = self
            .snapshot(job_id)
            .ok_or(DraftServiceError::InvalidAttempt(
                "unknown release preparation",
            ))?;
        let (PublishStage::ReadyToPublish, Some(prepared)) = (snapshot.stage, &snapshot.prepared)
        else {
            return Err(DraftServiceError::InvalidAttempt(
                "the release preparation is not ready to publish",
            ));
        };
        let mut workflow = PublishingWorkflow::restore(snapshot.stage);
        workflow
            .transition(PublishStage::Published)
            .map_err(|_| DraftServiceError::InvalidAttempt("the release cannot be published"))?;
        let record = drafts.activate_release(
            &self.staging_root,
            releases_root,
            prepared,
            &snapshot.draft_id,
            snapshot.draft_revision,
        )?;
        let published = self
            .update(job_id, |snapshot| {
                snapshot.stage = PublishStage::Published;
                snapshot.release_id = Some(record.release.release_id.clone());
            })
            .ok_or(DraftServiceError::InvalidAttempt(
                "unknown release preparation",
            ))?;
        Ok((record, published))
    }

    /// Register a new job for a frozen draft snapshot.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::JobActive`] when another job is running.
    pub fn begin(&self, draft: &DraftCourse) -> Result<PublishJobSnapshot, DraftServiceError> {
        let job_id = opaque_id("job")?;
        let mut registry = self.lock();
        if registry.active.is_some() {
            return Err(DraftServiceError::JobActive);
        }
        let snapshot = PublishJobSnapshot {
            job_id: job_id.clone(),
            draft_id: draft.draft_id.clone(),
            draft_revision: draft.revision,
            sequence: 0,
            stage: PublishStage::Draft,
            progress: None,
            prepared: None,
            failure: None,
            release_id: None,
        };
        registry.jobs.insert(
            job_id.clone(),
            Entry {
                snapshot: snapshot.clone(),
                cancellation: PackagingCancellation::default(),
                finished_order: None,
            },
        );
        registry.active = Some(job_id);
        Ok(snapshot)
    }

    /// Whether a job for `draft_id` is still preparing or is ready and
    /// waiting to be published.
    #[must_use]
    pub fn draft_in_progress(&self, draft_id: &str) -> bool {
        self.lock().jobs.values().any(|entry| {
            entry.snapshot.draft_id == draft_id
                && !matches!(
                    entry.snapshot.stage,
                    PublishStage::Published | PublishStage::Failed | PublishStage::Cancelled
                )
        })
    }

    /// Current snapshot of a job.
    #[must_use]
    pub fn snapshot(&self, job_id: &str) -> Option<PublishJobSnapshot> {
        self.lock()
            .jobs
            .get(job_id)
            .map(|entry| entry.snapshot.clone())
    }

    /// Request cancellation. Returns `false` for unknown or finished jobs.
    pub fn cancel(&self, job_id: &str) -> bool {
        self.lock().jobs.get(job_id).is_some_and(|entry| {
            let running = !entry.snapshot.is_finished();
            if running {
                entry.cancellation.cancel();
            }
            running
        })
    }

    /// Run a job registered with [`Self::begin`] to completion on the current
    /// (blocking) thread, reporting every stage change and throttled progress.
    pub fn run(
        &self,
        job_id: &str,
        draft: &DraftCourse,
        distribution: &DistributionOptions,
        notify: &mut dyn FnMut(&PublishJobSnapshot),
    ) -> Option<PublishJobSnapshot> {
        let cancellation = self.lock().jobs.get(job_id)?.cancellation.clone();
        let mut runner = Runner {
            jobs: self,
            job_id,
            workflow: PublishingWorkflow::default(),
            notify,
            last_progress: None,
        };
        let outcome = runner.prepare(draft, distribution, &cancellation);
        let final_snapshot = match outcome {
            Ok(prepared) => runner.advance(PublishStage::ReadyToPublish, |snapshot| {
                snapshot.prepared = Some(prepared);
            }),
            Err((error, attempt_id)) => {
                if let Some(attempt_id) = attempt_id {
                    let _ = discard_attempt(&self.staging_root, &attempt_id);
                }
                let failure = failure_for(&error);
                match failure {
                    None => runner.advance(PublishStage::Cancelled, |_| {}),
                    Some(failure) => runner.advance(PublishStage::Failed, |snapshot| {
                        snapshot.failure = Some(failure);
                    }),
                }
            }
        };
        self.finish(job_id);
        final_snapshot
    }

    fn update(
        &self,
        job_id: &str,
        change: impl FnOnce(&mut PublishJobSnapshot),
    ) -> Option<PublishJobSnapshot> {
        let mut registry = self.lock();
        let entry = registry.jobs.get_mut(job_id)?;
        change(&mut entry.snapshot);
        entry.snapshot.sequence += 1;
        Some(entry.snapshot.clone())
    }

    fn finish(&self, job_id: &str) {
        let mut registry = self.lock();
        registry.finished_counter += 1;
        let order = registry.finished_counter;
        if let Some(entry) = registry.jobs.get_mut(job_id) {
            entry.finished_order = Some(order);
        }
        if registry.active.as_deref() == Some(job_id) {
            registry.active = None;
        }
        let mut finished: Vec<(u64, String)> = registry
            .jobs
            .iter()
            .filter_map(|(id, entry)| entry.finished_order.map(|order| (order, id.clone())))
            .collect();
        if finished.len() > RETAINED_FINISHED_JOBS {
            finished.sort_unstable();
            let excess = finished.len() - RETAINED_FINISHED_JOBS;
            for (_, id) in finished.into_iter().take(excess) {
                registry.jobs.remove(&id);
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

struct Runner<'a, 'n> {
    jobs: &'a PublishJobs,
    job_id: &'a str,
    workflow: PublishingWorkflow,
    notify: &'n mut dyn FnMut(&PublishJobSnapshot),
    last_progress: Option<Instant>,
}

type StageError = (DraftServiceError, Option<String>);

impl Runner<'_, '_> {
    fn prepare(
        &mut self,
        draft: &DraftCourse,
        distribution: &DistributionOptions,
        cancellation: &PackagingCancellation,
    ) -> Result<PreparedRelease, StageError> {
        self.advance(PublishStage::Validating, |_| {});
        let issues = draft.validate();
        if !issues.is_empty() {
            return Err((
                DraftServiceError::Packaging(PackagingError::DraftInvalid(issues)),
                None,
            ));
        }
        self.advance(PublishStage::Packaging, |_| {});
        let jobs = self.jobs;
        let staging = stage_draft(
            draft,
            &jobs.staging_root,
            PackagingLimits::default(),
            cancellation,
            &mut |progress| {
                self.progress(progress.completed_bytes, progress.total_bytes);
            },
        )
        .map_err(|error| (error, None))?;
        let attempt_id = staging.attempt_id.clone();
        self.advance(PublishStage::GeneratingTorrent, |_| {});
        let mut verifying = false;
        let torrent = generate_torrent(
            &jobs.staging_root,
            &attempt_id,
            distribution,
            cancellation,
            &mut |phase, progress| {
                if phase == TorrentPhase::Verifying && !verifying {
                    verifying = true;
                    self.advance(PublishStage::Verifying, |_| {});
                }
                self.progress(progress.hashed_bytes, progress.total_bytes);
            },
        )
        .map_err(|error| (error, Some(attempt_id.clone())))?;
        if cancellation.is_cancelled() {
            return Err((
                DraftServiceError::Packaging(PackagingError::Cancelled),
                Some(attempt_id),
            ));
        }
        Ok(PreparedRelease { staging, torrent })
    }

    fn advance(
        &mut self,
        stage: PublishStage,
        change: impl FnOnce(&mut PublishJobSnapshot),
    ) -> Option<PublishJobSnapshot> {
        if self.workflow.transition(stage).is_err() {
            return self.jobs.snapshot(self.job_id);
        }
        let snapshot = self.jobs.update(self.job_id, |snapshot| {
            snapshot.stage = stage;
            snapshot.progress = None;
            change(snapshot);
        })?;
        self.last_progress = None;
        (self.notify)(&snapshot);
        Some(snapshot)
    }

    fn progress(&mut self, completed_bytes: u64, total_bytes: u64) {
        let now = Instant::now();
        let due = completed_bytes >= total_bytes
            || self
                .last_progress
                .is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL);
        if !due {
            return;
        }
        self.last_progress = Some(now);
        if let Some(snapshot) = self.jobs.update(self.job_id, |snapshot| {
            snapshot.progress = Some(JobProgress {
                completed_bytes,
                total_bytes,
            });
        }) {
            (self.notify)(&snapshot);
        }
    }
}

/// `None` means the job was cancelled rather than failed.
fn failure_for(error: &DraftServiceError) -> Option<JobFailure> {
    let code = match error {
        DraftServiceError::Packaging(PackagingError::Cancelled)
        | DraftServiceError::Torrent(TorrentError::Cancelled) => return None,
        DraftServiceError::Packaging(PackagingError::DraftInvalid(_)) => "draft_invalid",
        DraftServiceError::Packaging(PackagingError::Manifest(_)) => "manifest_invalid",
        DraftServiceError::Packaging(
            PackagingError::SourceUnavailable { .. } | PackagingError::SourceChanged { .. },
        ) => "source_changed",
        DraftServiceError::Packaging(PackagingError::LimitExceeded(_)) => "limit_exceeded",
        DraftServiceError::Torrent(TorrentError::Verification(_)) => "verification_failed",
        _ => "preparation_failed",
    };
    Some(JobFailure {
        code,
        message: error.to_string(),
    })
}
