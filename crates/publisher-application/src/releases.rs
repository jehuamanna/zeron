use std::{
    fs, io,
    path::{Path, PathBuf},
};

use course_publisher::{
    DraftCourse, DraftLesson, DraftMetadata, DraftModule, DraftResource, PackagingCancellation,
};
use publisher_storage::{NewRelease, ReleaseFiles, ReleaseRecord, ReleaseStatus, StoredDraft};
use torrent_metainfo::{
    HashProgress, PackageFile, TorrentError, file_table, info_hash, inspect_torrent, verify_torrent,
};

use crate::{
    ActivityKind, DraftServiceError, PreparedRelease, PublisherDrafts, opaque_id,
    torrents::{StagedAttempt, read_torrent},
};

const RELEASE_PREFIX: &str = "release_";
const PARTIAL_SUFFIX: &str = ".partial";
const DELETING_SUFFIX: &str = ".deleting";
/// Package directory inside a release directory.
pub const RELEASE_PACKAGE_DIRECTORY: &str = "package";
/// Torrent file inside a release directory.
pub const RELEASE_TORRENT_FILE: &str = "release.torrent";

impl PublisherDrafts {
    /// All release records, newest first.
    ///
    /// # Errors
    /// Returns a storage failure.
    pub fn releases(&self) -> Result<Vec<ReleaseRecord>, DraftServiceError> {
        Ok(self.storage.releases()?)
    }

    /// Turn a verified preparation into an immutable release.
    ///
    /// The staged attempt and its torrent are re-checked structurally (exact
    /// file tree, manifest digest, torrent file table, and info hash), moved
    /// by rename into `<releases_root>/<release>.partial/`, recorded, made
    /// read-only, and finally renamed to `<releases_root>/<release>/`. Before
    /// the record is committed every failure moves the artifacts back.
    ///
    /// # Errors
    /// Rejects reused versions, modified attempts, and I/O failures.
    pub fn activate_release(
        &mut self,
        staging_root: &Path,
        releases_root: &Path,
        prepared: &PreparedRelease,
        draft_id: &str,
        draft_revision: u64,
    ) -> Result<ReleaseRecord, DraftServiceError> {
        let attempt = StagedAttempt::open(staging_root, &prepared.staging.attempt_id)?;
        let torrent_path = attempt.torrent_path();
        let summary = inspect_torrent(&read_torrent(&torrent_path)?, &attempt.files)
            .map_err(DraftServiceError::Torrent)?;
        if summary.info_hash != prepared.torrent.info_hash
            || attempt.manifest_sha256 != prepared.staging.manifest_sha256
            || attempt.course_id != prepared.staging.course_id
            || attempt.course_version != prepared.staging.course_version
        {
            return Err(DraftServiceError::InvalidAttempt(
                "the prepared artifacts changed after verification",
            ));
        }
        if self
            .storage
            .version_released(&attempt.course_id, &attempt.course_version)?
        {
            return Err(DraftServiceError::VersionAlreadyReleased {
                course_version: attempt.course_version,
            });
        }

        let release_id = opaque_id("release")?;
        fs::create_dir_all(releases_root)
            .map_err(|source| io_error("creating the releases directory", source))?;
        let partial = releases_root.join(format!("{release_id}{PARTIAL_SUFFIX}"));
        fs::create_dir(&partial)
            .map_err(|source| io_error("creating the release directory", source))?;
        let package = partial.join(RELEASE_PACKAGE_DIRECTORY);
        let torrent = partial.join(RELEASE_TORRENT_FILE);
        let moved = fs::rename(&attempt.root, &package)
            .map_err(|source| io_error("moving the package into the release", source))
            .and_then(|()| {
                fs::rename(&torrent_path, &torrent).map_err(|source| {
                    let _ = fs::rename(&package, &attempt.root);
                    io_error("moving the torrent into the release", source)
                })
            });
        if let Err(error) = moved {
            let _ = fs::remove_dir(&partial);
            return Err(error);
        }

        let record = self.storage.insert_release(NewRelease {
            release_id: release_id.clone(),
            course_id: attempt.course_id.clone(),
            course_version: attempt.course_version.clone(),
            title: attempt.title.clone(),
            draft_id: draft_id.to_owned(),
            draft_revision,
            manifest_sha256: attempt.manifest_sha256.clone(),
            info_hash: summary.info_hash,
            magnet: prepared.torrent.magnet.clone(),
            piece_length: summary.piece_length,
            piece_count: summary.piece_count as u64,
            file_count: attempt.files.len() as u64,
            total_bytes: summary.total_length,
        });
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                let _ = fs::rename(&torrent, &torrent_path);
                let _ = fs::rename(&package, &attempt.root);
                let _ = fs::remove_dir(&partial);
                return Err(error.into());
            }
        };
        finalize(&partial, &releases_root.join(&release_id))?;
        self.record_activity(
            Some(&release_id),
            ActivityKind::ReleasePublished,
            &format!("Version {} published", record.release.course_version),
        );
        Ok(record)
    }

    /// Fully re-verify a recorded release: exact package tree, manifest digest,
    /// torrent structure, info hash, and every piece hash.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases or
    /// mismatched records and a torrent error for any byte that differs.
    pub fn verify_release(
        &self,
        releases_root: &Path,
        release_id: &str,
        cancellation: &PackagingCancellation,
        progress: &mut dyn FnMut(HashProgress),
    ) -> Result<ReleaseRecord, DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        let directory = release_directory(releases_root, release_id).ok_or_else(unknown)?;
        let record = self.storage.release(release_id)?.ok_or_else(unknown)?;
        let package = StagedAttempt::open_package(
            directory.join(RELEASE_PACKAGE_DIRECTORY),
            directory.join(RELEASE_TORRENT_FILE),
        )?;
        if package.manifest_sha256 != record.release.manifest_sha256 {
            return Err(DraftServiceError::InvalidAttempt(
                "the released course.json differs from its record",
            ));
        }
        let cancelled = || cancellation.is_cancelled();
        let verified = verify_torrent(
            &read_torrent(&package.torrent_path())?,
            &package.root,
            &package.files,
            &cancelled,
            progress,
        )
        .map_err(DraftServiceError::Torrent)?;
        if verified.info_hash != record.release.info_hash {
            return Err(DraftServiceError::InvalidAttempt(
                "the released torrent differs from its record",
            ));
        }
        Ok(record)
    }

    /// Mark a release archived and forget its seeding intent. The release
    /// files stay on disk untouched. Callers that run an engine must stop
    /// seeding first; [`crate::SeedingService::archive`] does both.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases or a
    /// storage failure.
    pub fn archive_release(
        &mut self,
        release_id: &str,
    ) -> Result<ReleaseRecord, DraftServiceError> {
        let before = self
            .storage
            .release(release_id)?
            .map(|record| record.status);
        let record = self
            .storage
            .archive_release(release_id)?
            .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
        if before != Some(ReleaseStatus::Archived) {
            self.record_activity(
                Some(release_id),
                ActivityKind::ReleaseArchived,
                &format!(
                    "Version {} archived; it is no longer seeded",
                    record.release.course_version
                ),
            );
        }
        Ok(record)
    }

    /// Make a superseded release the course's current release again, for
    /// example to roll back a newer version. The release that was current
    /// becomes superseded. Release bytes and seeding are unchanged.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases,
    /// [`DraftServiceError::ReleaseArchived`] for archived ones, or a storage
    /// failure.
    pub fn make_release_current(
        &mut self,
        release_id: &str,
    ) -> Result<ReleaseRecord, DraftServiceError> {
        let record = self
            .storage
            .release(release_id)?
            .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
        if record.status == ReleaseStatus::Archived {
            return Err(DraftServiceError::ReleaseArchived);
        }
        let current = self
            .storage
            .make_release_current(release_id)?
            .ok_or(DraftServiceError::ReleaseArchived)?;
        if record.status == ReleaseStatus::Superseded {
            self.record_activity(
                Some(release_id),
                ActivityKind::ReleaseMadeCurrent,
                &format!(
                    "Version {} is the current release again",
                    current.release.course_version
                ),
            );
        }
        Ok(current)
    }

    /// Start a new draft from a recorded release, for the course's next
    /// version.
    ///
    /// The release's `course.json` is re-read and validated together with
    /// the exact package tree. The draft keeps the course, module, lesson, and
    /// resource IDs, so learners' notes stay attached, and its sources are
    /// the release's own read-only package files, which are only ever read.
    /// The version is the next patch version when the released one is
    /// semantic, and otherwise the same version for the publisher to change.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases,
    /// releases whose files were deleted, or packages that no longer match
    /// their record, or a storage failure.
    pub fn draft_from_release(
        &mut self,
        releases_root: &Path,
        release_id: &str,
    ) -> Result<StoredDraft, DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        let directory = release_directory(releases_root, release_id).ok_or_else(unknown)?;
        let record = self.storage.release(release_id)?.ok_or_else(unknown)?;
        if record.files != ReleaseFiles::Present {
            return Err(DraftServiceError::InvalidAttempt(
                "the release files were deleted",
            ));
        }
        let package_root = directory.join(RELEASE_PACKAGE_DIRECTORY);
        let package = StagedAttempt::open_package(
            package_root.clone(),
            directory.join(RELEASE_TORRENT_FILE),
        )?;
        if package.manifest_sha256 != record.release.manifest_sha256 {
            return Err(DraftServiceError::InvalidAttempt(
                "the released course.json differs from its record",
            ));
        }
        let package_root = fs::canonicalize(&package_root)
            .map_err(|source| io_error("resolving the release package", source))?;
        let manifest = package.manifest;
        if manifest.access.is_some() {
            // Its paid resources are sealed in the package; reusing them as
            // sources would seal them twice. Planned for a later slice.
            return Err(DraftServiceError::InvalidAttempt(
                "a paid release cannot be reopened as a draft yet",
            ));
        }
        let draft = DraftCourse {
            draft_id: opaque_id("draft")?,
            revision: 0,
            metadata: DraftMetadata {
                course_id: manifest.course_id,
                version: next_patch_version(&manifest.version),
                title: manifest.title,
                description: manifest.description,
                publisher_name: manifest.publisher.name,
                language: manifest.language,
                category: manifest.category,
                tags: manifest.tags,
                prerequisites: manifest.prerequisites,
                license_name: manifest.license.map(|license| license.name),
            },
            modules: manifest
                .modules
                .into_iter()
                .map(|module| DraftModule {
                    id: module.id,
                    title: module.title,
                    lessons: module
                        .lessons
                        .into_iter()
                        .map(|lesson| DraftLesson {
                            id: lesson.id,
                            title: lesson.title,
                            estimated_duration_seconds: lesson
                                .estimated_duration_seconds
                                .map(whole_seconds),
                            resources: lesson
                                .resources
                                .into_iter()
                                .map(|resource| DraftResource {
                                    source_path: package_root.join(&resource.path),
                                    id: resource.id,
                                    kind: resource.kind,
                                    package_path: resource.path,
                                    media_type: resource.media_type,
                                    duration_seconds: resource.duration_seconds.map(whole_seconds),
                                    language: resource.language,
                                })
                                .collect(),
                            paid: false,
                        })
                        .collect(),
                })
                .collect(),
        };
        let summary = self.storage.create_draft(&draft)?;
        Ok(StoredDraft {
            draft,
            updated_at_unix_seconds: summary.updated_at_unix_seconds,
        })
    }

    /// Permanently remove an archived release's directory. The record stays
    /// as history, so the version can never be reused.
    ///
    /// `confirmation` must equal the release's course version, so a stray
    /// call cannot delete files. The deletion is recorded first, then the
    /// directory is renamed out of place and removed without following
    /// symbolic links. [`Self::recover_release_deletions`] finishes a
    /// deletion interrupted by a crash. Calling this again on a deleted
    /// release returns its record.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases,
    /// [`DraftServiceError::ReleaseNotArchived`] for releases still in use,
    /// [`DraftServiceError::InvalidInput`] for a wrong confirmation, or an
    /// I/O or storage failure.
    pub fn delete_release_files(
        &mut self,
        releases_root: &Path,
        release_id: &str,
        confirmation: &str,
    ) -> Result<ReleaseRecord, DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        release_directory(releases_root, release_id).ok_or_else(unknown)?;
        let record = self.storage.release(release_id)?.ok_or_else(unknown)?;
        if record.status != ReleaseStatus::Archived {
            return Err(DraftServiceError::ReleaseNotArchived);
        }
        if record.files == ReleaseFiles::Deleted {
            return Ok(record);
        }
        if confirmation.trim() != record.release.course_version {
            return Err(DraftServiceError::InvalidInput {
                field: "confirmation".to_owned(),
                message: "must match the course version",
            });
        }
        self.delete_archived_files(releases_root, &record)?;
        self.storage.release(release_id)?.ok_or_else(unknown)
    }

    /// Delete an archived release's files, recorded before and after so a
    /// crash is finished at the next start. The caller has checked consent.
    pub(crate) fn delete_archived_files(
        &mut self,
        releases_root: &Path,
        record: &ReleaseRecord,
    ) -> Result<(), DraftServiceError> {
        let release_id = record.release.release_id.as_str();
        self.storage
            .begin_release_file_deletion(release_id)?
            .ok_or(DraftServiceError::ReleaseNotArchived)?;
        remove_release_tree(releases_root, release_id)?;
        self.storage.finish_release_file_deletion(release_id)?;
        self.record_activity(
            Some(release_id),
            ActivityKind::ReleaseFilesDeleted,
            &format!(
                "Files of version {} deleted from this computer",
                record.release.course_version
            ),
        );
        Ok(())
    }

    /// Finish file deletions interrupted by a crash. Call once at startup,
    /// after [`Self::recover_releases`].
    ///
    /// # Errors
    /// Returns a storage or I/O failure; remaining deletions are retried on
    /// the next start.
    pub fn recover_release_deletions(
        &mut self,
        releases_root: &Path,
    ) -> Result<usize, DraftServiceError> {
        let pending = self.storage.releases_deleting_files()?;
        for release_id in &pending {
            if release_directory(releases_root, release_id).is_none() {
                continue;
            }
            remove_release_tree(releases_root, release_id)?;
            self.storage.finish_release_file_deletion(release_id)?;
        }
        Ok(pending.len())
    }

    /// The torrent of a recorded release and the files it declares.
    ///
    /// The bytes are read with a size bound and their info hash must match
    /// the record, so an export never hands out a torrent that differs from
    /// what was published. Declared paths are package-relative.
    ///
    /// # Errors
    /// Returns [`DraftServiceError::InvalidAttempt`] for unknown releases, a
    /// missing torrent, or one that no longer matches its record.
    pub fn release_torrent(
        &self,
        releases_root: &Path,
        release_id: &str,
    ) -> Result<ReleaseTorrent, DraftServiceError> {
        let unknown = || DraftServiceError::InvalidAttempt("unknown release");
        let directory = release_directory(releases_root, release_id).ok_or_else(unknown)?;
        let record = self.storage.release(release_id)?.ok_or_else(unknown)?;
        let bytes = read_torrent(&directory.join(RELEASE_TORRENT_FILE))?;
        if info_hash(&bytes).map_err(DraftServiceError::Torrent)? != record.release.info_hash {
            return Err(DraftServiceError::InvalidAttempt(
                "the released torrent differs from its record",
            ));
        }
        let files = file_table(&bytes).map_err(DraftServiceError::Torrent)?;
        Ok(ReleaseTorrent {
            record,
            bytes,
            files,
        })
    }

    /// Finish activations interrupted by a crash and discard ones that never
    /// reached their durable record. Call once at startup.
    ///
    /// # Errors
    /// Returns a storage or I/O failure.
    pub fn recover_releases(&self, releases_root: &Path) -> Result<usize, DraftServiceError> {
        let entries = match fs::read_dir(releases_root) {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(source) => return Err(io_error("listing the releases directory", source)),
        };
        let mut recovered = 0;
        for entry in entries {
            let entry =
                entry.map_err(|source| io_error("listing the releases directory", source))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(release_id) = name
                .strip_suffix(PARTIAL_SUFFIX)
                .filter(|id| is_release_id(id))
            else {
                continue;
            };
            let is_directory = entry
                .file_type()
                .map_err(|source| io_error("inspecting a release directory", source))?
                .is_dir();
            if !is_directory {
                continue;
            }
            let final_path = releases_root.join(release_id);
            if self.storage.release(release_id)?.is_some() && !final_path.exists() {
                finalize(&entry.path(), &final_path)?;
            } else {
                fs::remove_dir_all(entry.path())
                    .map_err(|source| io_error("removing an incomplete release", source))?;
            }
            recovered += 1;
        }
        Ok(recovered)
    }
}

/// A recorded release's verified torrent, for export and display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTorrent {
    /// The release the torrent belongs to.
    pub record: ReleaseRecord,
    /// Exact `.torrent` bytes as published.
    pub bytes: Vec<u8>,
    /// Declared files in torrent order, with package-relative paths.
    pub files: Vec<PackageFile>,
}

/// Location of a recorded release's package, for later seeding.
#[must_use]
pub fn release_directory(releases_root: &Path, release_id: &str) -> Option<PathBuf> {
    is_release_id(release_id).then(|| releases_root.join(release_id))
}

/// Move `<root>/<release_id>` aside to `<release_id>.deleting`, then remove
/// it. A symbolic link in either place is removed itself, never followed;
/// `fs::remove_dir_all` does not follow links inside the tree.
fn remove_release_tree(releases_root: &Path, release_id: &str) -> Result<(), DraftServiceError> {
    let target = release_directory(releases_root, release_id)
        .ok_or(DraftServiceError::InvalidAttempt("unknown release"))?;
    let aside = releases_root.join(format!("{release_id}{DELETING_SUFFIX}"));
    match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.is_dir() {
                ensure_inside(releases_root, &target)?;
            }
            if fs::symlink_metadata(&aside).is_ok() {
                remove_entry(&aside)?;
            }
            fs::rename(&target, &aside)
                .map_err(|source| io_error("moving the release aside for deletion", source))?;
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(io_error("inspecting the release directory", source)),
    }
    match fs::symlink_metadata(&aside) {
        Ok(_) => remove_entry(&aside),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("inspecting the release directory", source)),
    }
}

fn remove_entry(path: &Path) -> Result<(), DraftServiceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspecting the release directory", source))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|source| io_error("removing release files", source))
}

/// Re-check, after resolving every link, that a release directory sits
/// directly under the releases root.
fn ensure_inside(releases_root: &Path, target: &Path) -> Result<(), DraftServiceError> {
    let root = fs::canonicalize(releases_root)
        .map_err(|source| io_error("resolving the releases directory", source))?;
    let resolved = fs::canonicalize(target)
        .map_err(|source| io_error("resolving the release directory", source))?;
    if resolved.parent() == Some(root.as_path()) && resolved.file_name() == target.file_name() {
        Ok(())
    } else {
        Err(DraftServiceError::InvalidAttempt(
            "the release directory is outside the releases folder",
        ))
    }
}

fn finalize(partial: &Path, final_path: &Path) -> Result<(), DraftServiceError> {
    make_read_only(&partial.join(RELEASE_PACKAGE_DIRECTORY))?;
    make_read_only(&partial.join(RELEASE_TORRENT_FILE))?;
    fs::rename(partial, final_path).map_err(|source| io_error("activating the release", source))
}

fn make_read_only(path: &Path) -> Result<(), DraftServiceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspecting release files", source))?;
    if metadata.file_type().is_dir() {
        for entry in
            fs::read_dir(path).map_err(|source| io_error("listing release files", source))?
        {
            let entry = entry.map_err(|source| io_error("listing release files", source))?;
            make_read_only(&entry.path())?;
        }
    } else if metadata.file_type().is_file() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions)
            .map_err(|source| io_error("protecting release files", source))?;
    }
    Ok(())
}

fn is_release_id(value: &str) -> bool {
    value.strip_prefix(RELEASE_PREFIX).is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The next patch version of a semantic version, without pre-release or
/// build parts; any other version is returned unchanged.
fn next_patch_version(version: &str) -> String {
    match semver::Version::parse(version) {
        Ok(parsed) => {
            semver::Version::new(parsed.major, parsed.minor, parsed.patch + 1).to_string()
        }
        Err(_) => version.to_owned(),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "manifest durations are validated finite and non-negative"
)]
fn whole_seconds(seconds: f64) -> u64 {
    seconds.round() as u64
}

fn io_error(operation: &'static str, source: io::Error) -> DraftServiceError {
    DraftServiceError::Torrent(TorrentError::Io { operation, source })
}
