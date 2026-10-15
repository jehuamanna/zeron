use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use publisher_storage::{ReleaseRecord, ReleaseStatus};
use torrent_metainfo::inspect_torrent;
use torrent_port::{
    EngineError, InfoHash, NetworkProfile, PeerReachability, RateLimits, SeedRequest,
    TorrentEngine, TransferStats,
};

use crate::{
    DraftServiceError, PublisherDrafts, RELEASE_PACKAGE_DIRECTORY, RELEASE_TORRENT_FILE,
    release_directory,
    torrents::{StagedAttempt, read_torrent},
};

const NETWORK_PROFILE_KEY: &str = "network_profile";
const PEER_PORT_KEY: &str = "peer_port";
const PORT_FORWARDING_KEY: &str = "port_forwarding";
const UPLOAD_LIMIT_KEY: &str = "upload_limit_kib";

/// Stable storage and IPC name of a network profile.
#[must_use]
pub const fn network_profile_name(profile: NetworkProfile) -> &'static str {
    profile.name()
}

/// Parse a network profile name.
#[must_use]
pub fn parse_network_profile(value: &str) -> Option<NetworkProfile> {
    NetworkProfile::from_name(value)
}

impl PublisherDrafts {
    /// Configured network profile; local network unless the publisher chose otherwise.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn network_profile(&self) -> Result<NetworkProfile, DraftServiceError> {
        Ok(self
            .storage
            .setting(NETWORK_PROFILE_KEY)?
            .as_deref()
            .and_then(parse_network_profile)
            .unwrap_or(NetworkProfile::LocalNetwork))
    }

    /// Persist the network profile used the next time the engine starts.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn set_network_profile(
        &mut self,
        profile: NetworkProfile,
    ) -> Result<(), DraftServiceError> {
        Ok(self
            .storage
            .set_setting(NETWORK_PROFILE_KEY, network_profile_name(profile))?)
    }

    /// Configured peer port and router forwarding; an ephemeral port without
    /// forwarding unless the publisher chose otherwise.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn peer_reachability(&self) -> Result<PeerReachability, DraftServiceError> {
        Ok(PeerReachability::from_settings(
            self.storage.setting(PEER_PORT_KEY)?.as_deref(),
            self.storage.setting(PORT_FORWARDING_KEY)?.as_deref(),
        ))
    }

    /// Persist the peer port and forwarding used the next time the engine starts.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn set_peer_reachability(
        &mut self,
        reachability: PeerReachability,
    ) -> Result<(), DraftServiceError> {
        self.storage
            .set_setting(PEER_PORT_KEY, &reachability.port_setting())?;
        Ok(self
            .storage
            .set_setting(PORT_FORWARDING_KEY, reachability.port_forwarding_setting())?)
    }

    /// Stored seeding upload cap; unlimited unless the publisher chose one.
    /// The Admin only seeds, so it has no download cap.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn upload_limit(&self) -> Result<RateLimits, DraftServiceError> {
        Ok(RateLimits::from_settings(
            None,
            self.storage.setting(UPLOAD_LIMIT_KEY)?.as_deref(),
        ))
    }

    /// Persist the seeding upload cap; any download cap is ignored.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn set_upload_limit(&mut self, limits: RateLimits) -> Result<(), DraftServiceError> {
        let (_, upload) = limits.to_settings();
        Ok(self.storage.set_setting(UPLOAD_LIMIT_KEY, &upload)?)
    }

    /// One release record.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn release(&self, release_id: &str) -> Result<Option<ReleaseRecord>, DraftServiceError> {
        Ok(self.storage.release(release_id)?)
    }

    /// Build a seed request for a recorded release after a structural check:
    /// exact package tree, manifest digest, torrent file table, and info hash
    /// must all match the record. Piece hashes are checked by the engine
    /// before anything is served.
    ///
    /// # Errors
    /// Rejects unknown releases and releases whose files changed.
    pub fn seed_request(
        &self,
        releases_root: &Path,
        release_id: &str,
    ) -> Result<(ReleaseRecord, SeedRequest), DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        let directory = release_directory(releases_root, release_id).ok_or_else(unknown)?;
        let record = self.storage.release(release_id)?.ok_or_else(unknown)?;
        if record.status == ReleaseStatus::Archived {
            return Err(DraftServiceError::ReleaseArchived);
        }
        let package = StagedAttempt::open_package(
            directory.join(RELEASE_PACKAGE_DIRECTORY),
            directory.join(RELEASE_TORRENT_FILE),
        )?;
        let torrent = read_torrent(&package.torrent_path())?;
        let summary =
            inspect_torrent(&torrent, &package.files).map_err(DraftServiceError::Torrent)?;
        if package.manifest_sha256 != record.release.manifest_sha256
            || summary.info_hash != record.release.info_hash
        {
            return Err(DraftServiceError::InvalidAttempt(
                "the release files differ from their record",
            ));
        }
        Ok((
            record,
            SeedRequest {
                torrent,
                content_root: package.root,
            },
        ))
    }
}

/// A release with its persisted seeding intent and engine-local metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSeedingStatus {
    /// Release record.
    pub release: ReleaseRecord,
    /// Whether the publisher wants this release seeded.
    pub enabled: bool,
    /// Engine metrics while the release is active in this process.
    pub transfer: Option<TransferStats>,
    /// Why the last start attempt failed, if it did.
    pub start_error: Option<String>,
}

/// Starts, stops, and restores release seeding through a [`TorrentEngine`].
pub struct SeedingService<E> {
    drafts: Arc<Mutex<PublisherDrafts>>,
    engine: Arc<E>,
    releases_root: PathBuf,
    start_errors: Mutex<HashMap<String, String>>,
}

impl<E: TorrentEngine> SeedingService<E> {
    /// Compose the service.
    #[must_use]
    pub fn new(
        drafts: Arc<Mutex<PublisherDrafts>>,
        engine: Arc<E>,
        releases_root: PathBuf,
    ) -> Self {
        Self {
            drafts,
            engine,
            releases_root,
            start_errors: Mutex::new(HashMap::new()),
        }
    }

    /// Seed a release and remember the intent across restarts.
    ///
    /// # Errors
    /// Returns structural or engine failures; the intent is then left unchanged.
    pub async fn start(&self, release_id: &str) -> Result<(), DraftServiceError> {
        let request = self.drafts().seed_request(&self.releases_root, release_id);
        let request = match request {
            Ok((_, request)) => request,
            Err(error) => {
                self.remember_error(release_id, &error);
                return Err(error);
            }
        };
        match self.engine.seed(request).await {
            Ok(_) | Err(EngineError::AlreadyActive) => {}
            Err(error) => {
                let error = DraftServiceError::Seeding(error);
                self.remember_error(release_id, &error);
                return Err(error);
            }
        }
        self.errors().remove(release_id);
        self.drafts().storage.set_seeding_intent(release_id, true)?;
        Ok(())
    }

    /// Stop seeding a release and forget the intent. Files are untouched.
    ///
    /// # Errors
    /// Returns an error for unknown releases or engine failures.
    pub async fn stop(&self, release_id: &str) -> Result<(), DraftServiceError> {
        let record = self
            .drafts()
            .release(release_id)?
            .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
        let info_hash =
            InfoHash::parse(&record.release.info_hash).map_err(DraftServiceError::Seeding)?;
        match self.engine.stop(&info_hash).await {
            Ok(()) | Err(EngineError::NotActive) => {}
            Err(error) => return Err(DraftServiceError::Seeding(error)),
        }
        self.errors().remove(release_id);
        self.drafts()
            .storage
            .set_seeding_intent(release_id, false)?;
        Ok(())
    }

    /// Stop seeding a release, then mark it archived and forget its intent.
    /// Files are untouched. If the engine cannot stop the release, nothing
    /// is archived.
    ///
    /// # Errors
    /// Returns an error for unknown releases, engine failures, or storage failures.
    pub async fn archive(&self, release_id: &str) -> Result<ReleaseRecord, DraftServiceError> {
        self.stop(release_id).await?;
        let record = self.drafts().archive_release(release_id)?;
        self.errors().remove(release_id);
        Ok(record)
    }

    /// Start every release the publisher wants seeded. Individual failures are
    /// remembered for [`Self::statuses`] and do not stop the others.
    ///
    /// # Errors
    /// Returns a storage failure when intents cannot be read.
    pub async fn restore(&self) -> Result<usize, DraftServiceError> {
        let intents = self.drafts().storage.seeding_intents()?;
        let mut started = 0;
        for release_id in intents {
            if self.start(&release_id).await.is_ok() {
                started += 1;
            }
        }
        Ok(started)
    }

    /// Every release with its seeding intent and current metrics, newest first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn statuses(&self) -> Result<Vec<ReleaseSeedingStatus>, DraftServiceError> {
        let (releases, intents) = {
            let drafts = self.drafts();
            (drafts.releases()?, drafts.storage.seeding_intents()?)
        };
        let errors = self.errors().clone();
        Ok(releases
            .into_iter()
            .map(|release| {
                let transfer = InfoHash::parse(&release.release.info_hash)
                    .ok()
                    .and_then(|info_hash| self.engine.transfer(&info_hash));
                let release_id = &release.release.release_id;
                ReleaseSeedingStatus {
                    enabled: intents.contains(release_id),
                    start_error: errors.get(release_id).cloned(),
                    transfer,
                    release,
                }
            })
            .collect())
    }

    fn drafts(&self) -> MutexGuard<'_, PublisherDrafts> {
        self.drafts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn errors(&self) -> MutexGuard<'_, HashMap<String, String>> {
        self.start_errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn remember_error(&self, release_id: &str, error: &DraftServiceError) {
        self.errors()
            .insert(release_id.to_owned(), error.to_string());
    }
}
