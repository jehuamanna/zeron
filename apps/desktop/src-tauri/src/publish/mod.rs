//! Publish: drafts, the Builder, releases, and seeding them. Releases seed
//! from the app's public engine, the one that also serves shared courses.

pub(crate) mod catalog;
pub(crate) mod commands;
pub(crate) mod configuration;
pub(crate) mod drops;
pub(crate) mod identity;
pub(crate) mod tracker_lists;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use publisher_application::{
    DraftServiceError, DropInbox, InfoHash, NetworkProfile, PublishJobs, PublisherDrafts,
    ReleaseFiles, ReleaseRecord, SeedingService,
};
use torrent_librqbit::LibrqbitEngine;

use crate::Router;

pub(crate) struct PublishState {
    pub(crate) drafts: Arc<Mutex<PublisherDrafts>>,
    pub(crate) publish_jobs: Arc<PublishJobs>,
    pub(crate) releases_root: PathBuf,
    /// `None` when the public engine did not start; authoring still works.
    pub(crate) seeding: Option<Arc<SeedingService<LibrqbitEngine>>>,
    /// Scanned drops waiting for the publisher to apply them.
    pub(crate) drops: Arc<DropInbox>,
    /// The Library's router, told which torrents are releases so the
    /// learner's copy of one never moves onto the public engine.
    pub(crate) library_router: Option<Arc<Router>>,
    /// An imported configuration file waiting to be applied.
    pub(crate) pending_configuration: Mutex<configuration::PendingConfiguration>,
    /// The OS credential store holding the signing key, opened on first use.
    pub(crate) credentials: Arc<identity::Credentials>,
}

impl PublishState {
    /// Open the publisher's data below `app_data` and finish work a previous
    /// run left half done. Call before the Library restores its transfers,
    /// so every release is reserved on the public engine first.
    ///
    /// # Errors
    ///
    /// Returns storage or recovery failures that leave publishing unusable.
    pub(crate) fn open(
        app_data: &Path,
        public_engine: Option<Arc<LibrqbitEngine>>,
        library_router: Option<Arc<Router>>,
    ) -> Result<Self, DraftServiceError> {
        let mut drafts = PublisherDrafts::open(&app_data.join("publisher.sqlite3"))?;
        let releases_root = app_data.join("releases");
        drafts.recover_releases(&releases_root)?;
        // A failed retry leaves the release marked as deleting; the
        // publisher can retry from the UI, and the next start tries again.
        if let Err(error) = drafts.recover_release_deletions(&releases_root) {
            eprintln!("finishing interrupted release file deletions failed: {error}");
        }
        let publish_jobs = PublishJobs::new(&app_data.join("staging"));
        publish_jobs.recover()?;
        if let Some(router) = &library_router {
            for release in drafts.releases()? {
                reserve(router, &release);
            }
        }
        let drafts = Arc::new(Mutex::new(drafts));
        let seeding = public_engine.map(|engine| {
            Arc::new(SeedingService::new(
                Arc::clone(&drafts),
                engine,
                releases_root.clone(),
            ))
        });
        Ok(Self {
            drafts,
            publish_jobs: Arc::new(publish_jobs),
            releases_root,
            seeding,
            drops: Arc::new(DropInbox::default()),
            library_router,
            pending_configuration: Mutex::default(),
            credentials: Arc::default(),
        })
    }

    /// Reserve a newly activated release on the public engine.
    pub(crate) fn reserve(&self, release: &ReleaseRecord) {
        if let Some(router) = &self.library_router {
            reserve(router, release);
        }
    }

    /// Whether the Library shares this release's torrent from the public
    /// engine, so seeding it as a release would take over that transfer.
    pub(crate) fn shared_in_library(&self, release: &ReleaseRecord) -> bool {
        self.library_router.as_ref().is_some_and(|router| {
            InfoHash::parse(&release.release.info_hash)
                .is_ok_and(|info_hash| router.is_shared(&info_hash))
        })
    }

    /// Resume seeding the releases the publisher wants seeded and, under the
    /// Internet profile, refresh stale tracker lists. Fetching lists is
    /// internet traffic, so other profiles refresh only from the UI.
    pub(crate) fn spawn_background_work(&self, profile: NetworkProfile) {
        if let Some(seeding) = self.seeding.clone() {
            tauri::async_runtime::spawn(async move {
                let _ = seeding.restore().await;
            });
        }
        if profile == NetworkProfile::Internet {
            let drafts = Arc::clone(&self.drafts);
            tauri::async_runtime::spawn(async move {
                let _ =
                    commands::refresh_lists(drafts, Some(tracker_lists::STALE_AFTER_SECONDS)).await;
            });
        }
    }
}

/// Releases with files to seed are the public engine's; the Library keeps
/// its own copy of one private. Releases whose files are gone are not.
fn reserve(router: &Router, release: &ReleaseRecord) {
    if release.files != ReleaseFiles::Deleted
        && let Ok(info_hash) = InfoHash::parse(&release.release.info_hash)
    {
        router.reserve(info_hash);
    }
}
