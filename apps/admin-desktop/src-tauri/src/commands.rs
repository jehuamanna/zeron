use std::sync::{Arc, Mutex};

use course_format::ResourceKind;
use course_publisher::{DraftResource, DraftValidationIssue};
use publisher_application::{
    DraftServiceError, DraftSummary, FindingSeverity, MAX_TRACKERS, MetadataEdit, PeerReachability,
    PublishJobSnapshot, PublishJobs, PublishStage, PublisherDrafts, ReleaseRecord,
    ReleaseSeedingStatus, ReleaseTorrent, StoredDraft, StructureEdit, TransferState,
    inspect_draft_media, network_profile_name, parse_network_profile, source_info,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DraftSummaryDto {
    draft_id: String,
    course_id: String,
    course_version: String,
    title: String,
    revision: u64,
    updated_at_unix_seconds: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DraftDetailDto {
    draft_id: String,
    course_id: String,
    course_version: String,
    title: String,
    revision: u64,
    updated_at_unix_seconds: u64,
    description: Option<String>,
    publisher_name: String,
    language: String,
    category: Option<String>,
    tags: Vec<String>,
    prerequisites: Vec<String>,
    license_name: Option<String>,
    modules: Vec<ModuleDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleDto {
    id: String,
    title: String,
    lessons: Vec<LessonDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LessonDto {
    id: String,
    title: String,
    resources: Vec<ResourceDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceDto {
    id: String,
    kind: ResourceKind,
    file_name: String,
    media_type: String,
    byte_length: Option<u64>,
    duration_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaFindingDto {
    resource_id: String,
    field: String,
    file_name: String,
    code: &'static str,
    severity: &'static str,
    message: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftMetadataInput {
    course_version: String,
    title: String,
    description: Option<String>,
    publisher_name: String,
    language: String,
    category: Option<String>,
    tags: Vec<String>,
    prerequisites: Vec<String>,
    license_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum StructureEditInput {
    AddModule {
        title: String,
    },
    RenameModule {
        module_id: String,
        title: String,
    },
    RemoveModule {
        module_id: String,
    },
    MoveModule {
        module_id: String,
        position: usize,
    },
    AddLesson {
        module_id: String,
        title: String,
    },
    RenameLesson {
        lesson_id: String,
        title: String,
    },
    RemoveLesson {
        lesson_id: String,
    },
    MoveLesson {
        module_id: String,
        lesson_id: String,
        position: usize,
    },
    RemoveResource {
        resource_id: String,
    },
    MoveResource {
        lesson_id: String,
        resource_id: String,
        position: usize,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationIssueDto {
    code: &'static str,
    field: String,
    message: String,
}

pub(crate) const PUBLISH_JOB_EVENT: &str = "publish-job";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublishJobDto {
    job_id: String,
    draft_id: String,
    draft_revision: u64,
    sequence: u64,
    stage: PublishStage,
    progress: Option<JobProgressDto>,
    prepared: Option<PreparedReleaseDto>,
    failure: Option<JobFailureDto>,
    release_id: Option<String>,
}

/// A release plus the package-relative files its torrent declares.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseDetailsDto {
    release: ReleaseDto,
    files: Vec<ReleaseFileDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseFileDto {
    path: String,
    byte_length: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseDto {
    release_id: String,
    course_id: String,
    course_version: String,
    title: String,
    info_hash: String,
    magnet: String,
    piece_length: u64,
    piece_count: u64,
    file_count: u64,
    total_bytes: u64,
    published_at_unix_seconds: u64,
    status: &'static str,
    files: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobProgressDto {
    completed_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedReleaseDto {
    attempt_id: String,
    course_version: String,
    resource_count: usize,
    manifest_sha256: String,
    info_hash: String,
    magnet: String,
    tracker_count: usize,
    piece_length: u64,
    piece_count: usize,
    file_count: usize,
    total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobFailureDto {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DistributionDto {
    configured_profile: &'static str,
    active_profile: &'static str,
    listen_port: Option<u16>,
    /// Stored fixed peer port; `None` means a new port on each start.
    peer_port: Option<u16>,
    /// Stored `UPnP` choice, honored under the Internet profile only.
    port_forwarding: bool,
    /// Whether this session asks the router to forward its peer port.
    active_port_forwarding: bool,
    /// The stored network choice differs from the running engine's.
    restart_required: bool,
    /// Trackers written into new releases.
    trackers: Vec<String>,
    engine_error: Option<String>,
    releases: Vec<SeedingStatusDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeedingStatusDto {
    release_id: String,
    course_version: String,
    title: String,
    release_status: &'static str,
    enabled: bool,
    state: Option<&'static str>,
    verified_bytes: u64,
    total_bytes: u64,
    uploaded_bytes: u64,
    upload_bytes_per_second: u64,
    connected_peers: u32,
    failure: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_revision: Option<u64>,
}

#[tauri::command]
pub(crate) async fn list_drafts(
    state: State<'_, AppState>,
) -> Result<Vec<DraftSummaryDto>, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || list(&*lock_drafts(&drafts)?)).await
}

#[tauri::command]
pub(crate) async fn create_draft(
    state: State<'_, AppState>,
) -> Result<DraftDetailDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || create(&mut *lock_drafts(&drafts)?)).await
}

#[tauri::command]
pub(crate) async fn get_draft(
    draft_id: String,
    state: State<'_, AppState>,
) -> Result<Option<DraftDetailDto>, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || detail(&*lock_drafts(&drafts)?, &draft_id)).await
}

#[tauri::command]
pub(crate) async fn update_draft_metadata(
    draft_id: String,
    expected_revision: u64,
    metadata: DraftMetadataInput,
    state: State<'_, AppState>,
) -> Result<DraftDetailDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        update_metadata(
            &mut *lock_drafts(&drafts)?,
            &draft_id,
            expected_revision,
            metadata,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn edit_draft_structure(
    draft_id: String,
    expected_revision: u64,
    edit: StructureEditInput,
    state: State<'_, AppState>,
) -> Result<DraftDetailDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        edit_structure(
            &mut *lock_drafts(&drafts)?,
            &draft_id,
            expected_revision,
            edit,
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn choose_and_attach_resource(
    app: AppHandle,
    draft_id: String,
    expected_revision: u64,
    lesson_id: String,
    state: State<'_, AppState>,
) -> Result<Option<DraftDetailDto>, CommandError> {
    let selected = run_blocking(move || Ok(app.dialog().file().blocking_pick_file())).await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.into_path().map_err(|_| {
        CommandError::new(
            "unsupported_file_location",
            "the selected file location is not supported",
        )
    })?;
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        lock_drafts(&drafts)?
            .attach_source(&draft_id, expected_revision, &lesson_id, &source)
            .map(|stored| Some(DraftDetailDto::from(stored)))
            .map_err(|error| service_error("draft_save_failed", error))
    })
    .await
}

#[tauri::command]
pub(crate) async fn inspect_media(
    draft_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MediaFindingDto>, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        let snapshot = lock_drafts(&drafts)?
            .draft(&draft_id)
            .map_err(|error| service_error("draft_lookup_failed", error))?
            .ok_or_else(|| CommandError::new("draft_not_found", "the draft was not found"))?
            .draft;
        Ok(media_findings(&snapshot))
    })
    .await
}

#[tauri::command]
pub(crate) async fn start_publish_preparation(
    app: AppHandle,
    draft_id: String,
    state: State<'_, AppState>,
) -> Result<PublishJobDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let jobs = Arc::clone(&state.publish_jobs);
    let (draft, distribution, started) = run_blocking(move || {
        let drafts = lock_drafts(&drafts)?;
        let draft = drafts
            .draft(&draft_id)
            .map_err(|error| service_error("draft_lookup_failed", error))?
            .ok_or_else(|| CommandError::new("draft_not_found", "the draft was not found"))?
            .draft;
        let distribution = drafts
            .distribution_options()
            .map_err(|error| service_error("publish_start_failed", error))?;
        let started = jobs
            .begin(&draft)
            .map_err(|error| service_error("publish_start_failed", error))?;
        Ok((draft, distribution, started))
    })
    .await?;
    let jobs = Arc::clone(&state.publish_jobs);
    let job_id = started.job_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        jobs.run(&job_id, &draft, &distribution, &mut |snapshot| {
            let _ = app.emit(PUBLISH_JOB_EVENT, PublishJobDto::from(snapshot));
        });
    });
    Ok(PublishJobDto::from(&started))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn get_publish_job(job_id: String, state: State<'_, AppState>) -> Option<PublishJobDto> {
    state
        .publish_jobs
        .snapshot(&job_id)
        .as_ref()
        .map(PublishJobDto::from)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn cancel_publish_job(job_id: String, state: State<'_, AppState>) -> bool {
    state.publish_jobs.cancel(&job_id)
}

#[tauri::command]
pub(crate) async fn activate_release(
    app: AppHandle,
    job_id: String,
    state: State<'_, AppState>,
) -> Result<ReleaseDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let jobs = Arc::clone(&state.publish_jobs);
    let releases_root = state.releases_root.clone();
    let (record, snapshot) = run_blocking(move || {
        jobs.activate(&job_id, &mut *lock_drafts(&drafts)?, &releases_root)
            .map_err(|error| service_error("release_activation_failed", error))
    })
    .await?;
    let _ = app.emit(PUBLISH_JOB_EVENT, PublishJobDto::from(&snapshot));
    Ok(ReleaseDto::from(record))
}

#[tauri::command]
pub(crate) async fn list_releases(
    state: State<'_, AppState>,
) -> Result<Vec<ReleaseDto>, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        lock_drafts(&drafts)?
            .releases()
            .map(|records| records.into_iter().map(ReleaseDto::from).collect())
            .map_err(|error| service_error("release_list_failed", error))
    })
    .await
}

/// Delete a draft the editor last saw at `expected_revision`. Releases and
/// attached source files are never touched.
#[tauri::command]
pub(crate) async fn delete_draft(
    draft_id: String,
    expected_revision: u64,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let jobs = Arc::clone(&state.publish_jobs);
    run_blocking(move || {
        delete(
            &mut *lock_drafts(&drafts)?,
            &jobs,
            &draft_id,
            expected_revision,
        )
    })
    .await
}

/// Stop seeding a release and mark it archived. Its files stay on disk.
#[tauri::command]
pub(crate) async fn archive_release(
    release_id: String,
    state: State<'_, AppState>,
) -> Result<ReleaseDto, CommandError> {
    if let Some(seeding) = state.distribution.seeding.clone() {
        return seeding
            .archive(&release_id)
            .await
            .map(ReleaseDto::from)
            .map_err(|error| service_error("release_archive_failed", error));
    }
    // Without an engine nothing is being seeded in this process.
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || archive(&mut *lock_drafts(&drafts)?, &release_id)).await
}

/// Permanently remove an archived release's files. `confirmation` must be
/// the release's course version; the record stays as history.
#[tauri::command]
pub(crate) async fn delete_release_files(
    release_id: String,
    confirmation: String,
    state: State<'_, AppState>,
) -> Result<ReleaseDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let releases_root = state.releases_root.clone();
    run_blocking(move || {
        delete_files(
            &mut *lock_drafts(&drafts)?,
            &releases_root,
            &release_id,
            &confirmation,
        )
    })
    .await
}

fn delete_files(
    drafts: &mut PublisherDrafts,
    releases_root: &std::path::Path,
    release_id: &str,
    confirmation: &str,
) -> Result<ReleaseDto, CommandError> {
    drafts
        .delete_release_files(releases_root, release_id, confirmation)
        .map(ReleaseDto::from)
        .map_err(|error| service_error("release_delete_failed", error))
}

fn delete(
    drafts: &mut PublisherDrafts,
    jobs: &PublishJobs,
    draft_id: &str,
    expected_revision: u64,
) -> Result<(), CommandError> {
    if jobs.draft_in_progress(draft_id) {
        return Err(service_error(
            "draft_delete_failed",
            DraftServiceError::DraftBusy,
        ));
    }
    drafts
        .delete_draft(draft_id, expected_revision)
        .map_err(|error| service_error("draft_delete_failed", error))
}

fn archive(drafts: &mut PublisherDrafts, release_id: &str) -> Result<ReleaseDto, CommandError> {
    drafts
        .archive_release(release_id)
        .map(ReleaseDto::from)
        .map_err(|error| service_error("release_archive_failed", error))
}

#[tauri::command]
pub(crate) async fn get_release_details(
    release_id: String,
    state: State<'_, AppState>,
) -> Result<ReleaseDetailsDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let releases_root = state.releases_root.clone();
    run_blocking(move || {
        release_torrent(&*lock_drafts(&drafts)?, &releases_root, &release_id)
            .map(ReleaseDetailsDto::from)
    })
    .await
}

/// Save a copy of a release's `.torrent` where the publisher chooses.
///
/// Only the release id crosses IPC; the destination comes from a native save
/// dialog and stays in Rust. Returns `false` when the dialog is cancelled.
#[tauri::command]
pub(crate) async fn export_release_torrent(
    app: AppHandle,
    release_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let releases_root = state.releases_root.clone();
    let torrent =
        run_blocking(move || release_torrent(&*lock_drafts(&drafts)?, &releases_root, &release_id))
            .await?;
    let file_name = torrent_file_name(
        &torrent.record.release.title,
        &torrent.record.release.course_version,
    );
    let selected = run_blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_file_name(file_name)
            .add_filter("BitTorrent file", &["torrent"])
            .blocking_save_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let destination = selected.into_path().map_err(|_| {
        CommandError::new(
            "unsupported_file_location",
            "the selected file location is not supported",
        )
    })?;
    run_blocking(move || write_export(&destination, &torrent.bytes)).await?;
    Ok(true)
}

fn release_torrent(
    drafts: &PublisherDrafts,
    releases_root: &std::path::Path,
    release_id: &str,
) -> Result<ReleaseTorrent, CommandError> {
    drafts
        .release_torrent(releases_root, release_id)
        .map_err(|error| service_error("release_unavailable", error))
}

/// Write a fresh, writable file rather than copying the read-only original.
fn write_export(destination: &std::path::Path, bytes: &[u8]) -> Result<(), CommandError> {
    std::fs::write(destination, bytes).map_err(|_| {
        CommandError::new(
            "export_failed",
            "the torrent file could not be written to the chosen location",
        )
    })
}

/// A portable suggested name such as `rust-systems-1.2.0.torrent`.
fn torrent_file_name(title: &str, course_version: &str) -> String {
    let slug = |value: &str| {
        let mut slug = String::new();
        for character in value.chars() {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_') {
                slug.push(character.to_ascii_lowercase());
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        slug.trim_matches(|character| matches!(character, '-' | '.'))
            .chars()
            .take(64)
            .collect::<String>()
    };
    let title = slug(title);
    let title = if title.is_empty() {
        "course".to_owned()
    } else {
        title
    };
    let version = slug(course_version);
    if version.is_empty() {
        format!("{title}.torrent")
    } else {
        format!("{title}-{version}.torrent")
    }
}

#[tauri::command]
pub(crate) async fn get_distribution(
    state: State<'_, AppState>,
) -> Result<DistributionDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let seeding = state.distribution.seeding.clone();
    let active = (
        state.distribution.active_profile,
        state.distribution.active_reachability,
    );
    let active_profile = network_profile_name(active.0);
    let listen_port = state
        .distribution
        .engine
        .as_ref()
        .and_then(|engine| torrent_engine_port(engine));
    let engine_error = state.distribution.engine_error.clone();
    run_blocking(move || {
        let (configured, reachability, trackers) = {
            let drafts = lock_drafts(&drafts)?;
            let failed = |error| service_error("distribution_failed", error);
            (
                drafts.network_profile().map_err(failed)?,
                drafts.peer_reachability().map_err(failed)?,
                drafts.trackers().map_err(failed)?,
            )
        };
        let releases = match seeding {
            Some(seeding) => seeding
                .statuses()
                .map_err(|error| service_error("distribution_failed", error))?
                .into_iter()
                .map(SeedingStatusDto::from)
                .collect(),
            None => lock_drafts(&drafts)?
                .releases()
                .map_err(|error| service_error("distribution_failed", error))?
                .into_iter()
                .map(|release| {
                    SeedingStatusDto::from(ReleaseSeedingStatus {
                        release,
                        enabled: false,
                        transfer: None,
                        start_error: None,
                    })
                })
                .collect(),
        };
        Ok(DistributionDto {
            configured_profile: network_profile_name(configured),
            active_profile,
            listen_port,
            peer_port: reachability.port,
            port_forwarding: reachability.port_forwarding,
            active_port_forwarding: active.1.port_forwarding,
            restart_required: (configured, reachability.for_profile(configured)) != active,
            trackers,
            engine_error,
            releases,
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn start_seeding(
    release_id: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    seeding_service(&state)?
        .start(&release_id)
        .await
        .map_err(|error| service_error("seeding_failed", error))
}

#[tauri::command]
pub(crate) async fn stop_seeding(
    release_id: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    seeding_service(&state)?
        .stop(&release_id)
        .await
        .map_err(|error| service_error("seeding_failed", error))
}

#[tauri::command]
pub(crate) async fn set_network_profile(
    profile: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let profile = parse_network_profile(&profile)
        .ok_or_else(|| CommandError::new("invalid_input", "unknown network profile"))?;
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        lock_drafts(&drafts)?
            .set_network_profile(profile)
            .map_err(|error| service_error("settings_failed", error))
    })
    .await
}

/// Store the trackers written into new releases; returns the normalized list.
#[tauri::command]
pub(crate) async fn set_trackers(
    trackers: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, CommandError> {
    if trackers.len() > MAX_TRACKERS * 4 {
        return Err(CommandError {
            field: Some("trackers".to_owned()),
            ..CommandError::new("invalid_input", "too many tracker lines")
        });
    }
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        lock_drafts(&drafts)?
            .set_trackers(&trackers)
            .map_err(|error| service_error("settings_failed", error))
    })
    .await
}

/// Store the peer port and `UPnP` choice used the next time the engine starts.
#[tauri::command]
pub(crate) async fn set_peer_reachability(
    peer_port: Option<u16>,
    port_forwarding: bool,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let reachability = parse_reachability(peer_port, port_forwarding)?;
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || {
        lock_drafts(&drafts)?
            .set_peer_reachability(reachability)
            .map_err(|error| service_error("settings_failed", error))
    })
    .await
}

fn parse_reachability(
    peer_port: Option<u16>,
    port_forwarding: bool,
) -> Result<PeerReachability, CommandError> {
    PeerReachability::new(peer_port, port_forwarding).ok_or_else(|| CommandError {
        field: Some("peerPort".to_owned()),
        ..CommandError::new(
            "invalid_input",
            format_args!(
                "choose a port from {} to 65535",
                PeerReachability::MIN_FIXED_PORT
            ),
        )
    })
}

fn seeding_service(
    state: &State<'_, AppState>,
) -> Result<
    Arc<publisher_application::SeedingService<torrent_librqbit::LibrqbitEngine>>,
    CommandError,
> {
    state.distribution.seeding.clone().ok_or_else(|| {
        CommandError::new(
            "engine_unavailable",
            "the torrent engine did not start; restart Zeron Publisher",
        )
    })
}

fn torrent_engine_port(engine: &torrent_librqbit::LibrqbitEngine) -> Option<u16> {
    use publisher_application::TorrentEngine as _;
    engine.listen_address().map(|address| address.port())
}

#[tauri::command]
pub(crate) async fn validate_draft(
    draft_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ValidationIssueDto>, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    run_blocking(move || validate(&*lock_drafts(&drafts)?, &draft_id)).await
}

fn list(drafts: &PublisherDrafts) -> Result<Vec<DraftSummaryDto>, CommandError> {
    drafts
        .list()
        .map(|summaries| summaries.into_iter().map(DraftSummaryDto::from).collect())
        .map_err(|error| service_error("draft_list_failed", error))
}

fn create(drafts: &mut PublisherDrafts) -> Result<DraftDetailDto, CommandError> {
    drafts
        .create()
        .map(DraftDetailDto::from)
        .map_err(|error| service_error("draft_create_failed", error))
}

fn detail(
    drafts: &PublisherDrafts,
    draft_id: &str,
) -> Result<Option<DraftDetailDto>, CommandError> {
    drafts
        .draft(draft_id)
        .map(|stored| stored.map(DraftDetailDto::from))
        .map_err(|error| service_error("draft_lookup_failed", error))
}

fn update_metadata(
    drafts: &mut PublisherDrafts,
    draft_id: &str,
    expected_revision: u64,
    metadata: DraftMetadataInput,
) -> Result<DraftDetailDto, CommandError> {
    drafts
        .update_metadata(draft_id, expected_revision, metadata.into())
        .map(DraftDetailDto::from)
        .map_err(|error| service_error("draft_save_failed", error))
}

fn edit_structure(
    drafts: &mut PublisherDrafts,
    draft_id: &str,
    expected_revision: u64,
    edit: StructureEditInput,
) -> Result<DraftDetailDto, CommandError> {
    drafts
        .edit_structure(draft_id, expected_revision, edit.into())
        .map(DraftDetailDto::from)
        .map_err(|error| service_error("draft_save_failed", error))
}

fn validate(
    drafts: &PublisherDrafts,
    draft_id: &str,
) -> Result<Vec<ValidationIssueDto>, CommandError> {
    drafts
        .validate(draft_id)
        .map(|issues| issues.into_iter().map(ValidationIssueDto::from).collect())
        .map_err(|error| service_error("draft_validation_failed", error))
}

fn media_findings(draft: &course_publisher::DraftCourse) -> Vec<MediaFindingDto> {
    inspect_draft_media(draft)
        .into_iter()
        .flat_map(|resource| {
            resource
                .findings
                .into_iter()
                .map(move |finding| MediaFindingDto {
                    resource_id: resource.resource_id.clone(),
                    field: resource.field.clone(),
                    file_name: resource.file_name.clone(),
                    code: finding.code,
                    severity: match finding.severity {
                        FindingSeverity::Warning => "warning",
                        FindingSeverity::Advice => "advice",
                    },
                    message: finding.message,
                })
        })
        .collect()
}

impl From<&PublishJobSnapshot> for PublishJobDto {
    fn from(snapshot: &PublishJobSnapshot) -> Self {
        Self {
            job_id: snapshot.job_id.clone(),
            draft_id: snapshot.draft_id.clone(),
            draft_revision: snapshot.draft_revision,
            sequence: snapshot.sequence,
            stage: snapshot.stage,
            progress: snapshot.progress.map(|progress| JobProgressDto {
                completed_bytes: progress.completed_bytes,
                total_bytes: progress.total_bytes,
            }),
            prepared: snapshot
                .prepared
                .as_ref()
                .map(|prepared| PreparedReleaseDto {
                    attempt_id: prepared.staging.attempt_id.clone(),
                    course_version: prepared.staging.course_version.clone(),
                    resource_count: prepared.staging.resource_count,
                    manifest_sha256: prepared.staging.manifest_sha256.clone(),
                    info_hash: prepared.torrent.info_hash.clone(),
                    magnet: prepared.torrent.magnet.clone(),
                    tracker_count: prepared.torrent.tracker_count,
                    piece_length: prepared.torrent.piece_length,
                    piece_count: prepared.torrent.piece_count,
                    file_count: prepared.torrent.file_count,
                    total_bytes: prepared.torrent.total_bytes,
                }),
            failure: snapshot.failure.as_ref().map(|failure| JobFailureDto {
                code: failure.code,
                message: failure.message.clone(),
            }),
            release_id: snapshot.release_id.clone(),
        }
    }
}

impl From<ReleaseSeedingStatus> for SeedingStatusDto {
    fn from(status: ReleaseSeedingStatus) -> Self {
        let transfer = status.transfer;
        let release = status.release.release;
        Self {
            release_id: release.release_id,
            course_version: release.course_version,
            title: release.title,
            release_status: status.release.status.as_str(),
            enabled: status.enabled,
            state: transfer.as_ref().map(|transfer| match transfer.state {
                TransferState::Checking | TransferState::Downloading => "checking",
                TransferState::Seeding => "seeding",
                TransferState::Failed => "failed",
            }),
            verified_bytes: transfer
                .as_ref()
                .map_or(0, |transfer| transfer.verified_bytes),
            total_bytes: transfer
                .as_ref()
                .map_or(release.total_bytes, |transfer| transfer.total_bytes),
            uploaded_bytes: transfer
                .as_ref()
                .map_or(0, |transfer| transfer.uploaded_bytes),
            upload_bytes_per_second: transfer
                .as_ref()
                .map_or(0, |transfer| transfer.upload_bytes_per_second),
            connected_peers: transfer
                .as_ref()
                .map_or(0, |transfer| transfer.connected_peers),
            failure: transfer
                .and_then(|transfer| transfer.failure)
                .or(status.start_error),
        }
    }
}

impl From<ReleaseTorrent> for ReleaseDetailsDto {
    fn from(torrent: ReleaseTorrent) -> Self {
        Self {
            release: ReleaseDto::from(torrent.record),
            files: torrent
                .files
                .into_iter()
                .map(|file| ReleaseFileDto {
                    path: file.path,
                    byte_length: file.length,
                })
                .collect(),
        }
    }
}

impl From<ReleaseRecord> for ReleaseDto {
    fn from(record: ReleaseRecord) -> Self {
        let release = record.release;
        Self {
            release_id: release.release_id,
            course_id: release.course_id,
            course_version: release.course_version,
            title: release.title,
            info_hash: release.info_hash,
            magnet: release.magnet,
            piece_length: release.piece_length,
            piece_count: release.piece_count,
            file_count: release.file_count,
            total_bytes: release.total_bytes,
            published_at_unix_seconds: record.published_at_unix_seconds,
            status: record.status.as_str(),
            files: record.files.as_str(),
        }
    }
}

impl From<DraftMetadataInput> for MetadataEdit {
    fn from(input: DraftMetadataInput) -> Self {
        Self {
            version: input.course_version,
            title: input.title,
            description: input.description,
            publisher_name: input.publisher_name,
            language: input.language,
            category: input.category,
            tags: input.tags,
            prerequisites: input.prerequisites,
            license_name: input.license_name,
        }
    }
}

impl From<StructureEditInput> for StructureEdit {
    fn from(input: StructureEditInput) -> Self {
        match input {
            StructureEditInput::AddModule { title } => Self::AddModule { title },
            StructureEditInput::RenameModule { module_id, title } => {
                Self::RenameModule { module_id, title }
            }
            StructureEditInput::RemoveModule { module_id } => Self::RemoveModule { module_id },
            StructureEditInput::MoveModule {
                module_id,
                position,
            } => Self::MoveModule {
                module_id,
                position,
            },
            StructureEditInput::AddLesson { module_id, title } => {
                Self::AddLesson { module_id, title }
            }
            StructureEditInput::RenameLesson { lesson_id, title } => {
                Self::RenameLesson { lesson_id, title }
            }
            StructureEditInput::RemoveLesson { lesson_id } => Self::RemoveLesson { lesson_id },
            StructureEditInput::MoveLesson {
                module_id,
                lesson_id,
                position,
            } => Self::MoveLesson {
                module_id,
                lesson_id,
                position,
            },
            StructureEditInput::RemoveResource { resource_id } => {
                Self::RemoveResource { resource_id }
            }
            StructureEditInput::MoveResource {
                lesson_id,
                resource_id,
                position,
            } => Self::MoveResource {
                lesson_id,
                resource_id,
                position,
            },
        }
    }
}

impl From<DraftValidationIssue> for ValidationIssueDto {
    fn from(issue: DraftValidationIssue) -> Self {
        Self {
            code: issue.code,
            field: issue.field,
            message: issue.message,
        }
    }
}

impl From<DraftSummary> for DraftSummaryDto {
    fn from(summary: DraftSummary) -> Self {
        Self {
            draft_id: summary.draft_id,
            course_id: summary.course_id,
            course_version: summary.course_version,
            title: summary.title,
            revision: summary.revision,
            updated_at_unix_seconds: summary.updated_at_unix_seconds,
        }
    }
}

impl From<StoredDraft> for DraftDetailDto {
    fn from(stored: StoredDraft) -> Self {
        let draft = stored.draft;
        Self {
            draft_id: draft.draft_id,
            course_id: draft.metadata.course_id,
            course_version: draft.metadata.version,
            title: draft.metadata.title,
            revision: draft.revision,
            updated_at_unix_seconds: stored.updated_at_unix_seconds,
            description: draft.metadata.description,
            publisher_name: draft.metadata.publisher_name,
            language: draft.metadata.language,
            category: draft.metadata.category,
            tags: draft.metadata.tags,
            prerequisites: draft.metadata.prerequisites,
            license_name: draft.metadata.license_name,
            modules: draft
                .modules
                .into_iter()
                .map(|module| ModuleDto {
                    id: module.id,
                    title: module.title,
                    lessons: module
                        .lessons
                        .into_iter()
                        .map(|lesson| LessonDto {
                            id: lesson.id,
                            title: lesson.title,
                            resources: lesson.resources.iter().map(ResourceDto::from).collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<&DraftResource> for ResourceDto {
    fn from(resource: &DraftResource) -> Self {
        let source = source_info(resource);
        Self {
            id: resource.id.clone(),
            kind: resource.kind,
            file_name: source.file_name,
            media_type: resource.media_type.clone(),
            byte_length: source.byte_length,
            duration_seconds: resource.duration_seconds,
        }
    }
}

pub(crate) async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::new("worker_failed", error))?
}

pub(crate) fn lock_drafts(
    drafts: &Arc<Mutex<PublisherDrafts>>,
) -> Result<std::sync::MutexGuard<'_, PublisherDrafts>, CommandError> {
    drafts.lock().map_err(|_| {
        CommandError::new(
            "storage_unavailable",
            "the publisher storage worker is unavailable",
        )
    })
}

pub(crate) fn service_error(code: &'static str, error: DraftServiceError) -> CommandError {
    match error {
        DraftServiceError::RevisionConflict { actual, .. } => CommandError {
            current_revision: Some(actual),
            ..CommandError::new("revision_conflict", error)
        },
        DraftServiceError::NotFound { .. } => CommandError::new("draft_not_found", error),
        DraftServiceError::EntityNotFound { .. } => CommandError::new("entity_not_found", error),
        DraftServiceError::InvalidPosition { .. } => CommandError::new("invalid_position", error),
        DraftServiceError::InvalidInput { ref field, .. } => CommandError {
            field: Some(field.clone()),
            ..CommandError::new("invalid_input", error)
        },
        DraftServiceError::SourceRejected(_) => CommandError::new("source_rejected", error),
        DraftServiceError::IdGeneration(_)
        | DraftServiceError::Storage(_)
        | DraftServiceError::Packaging(_)
        | DraftServiceError::Torrent(_)
        | DraftServiceError::InvalidAttempt(_) => CommandError::new(code, error),
        DraftServiceError::JobActive => CommandError::new("job_active", error),
        DraftServiceError::DraftBusy => CommandError::new("draft_busy", error),
        DraftServiceError::ReleaseArchived => CommandError::new("release_archived", error),
        DraftServiceError::ReleaseNotArchived => CommandError::new("release_not_archived", error),
        DraftServiceError::Seeding(_) => CommandError::new("seeding_failed", error),
        DraftServiceError::VersionAlreadyReleased { .. } => {
            CommandError::new("version_already_released", error)
        }
    }
}

impl CommandError {
    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn new(code: &'static str, message: impl std::fmt::Display) -> Self {
        Self {
            code,
            message: message.to_string(),
            field: None,
            current_revision: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use course_format::ResourceKind;
    use course_publisher::{DraftLesson, DraftModule, DraftResource};

    use publisher_application::DistributionOptions;

    use super::*;

    fn input(title: &str) -> DraftMetadataInput {
        serde_json::from_value(serde_json::json!({
            "courseVersion": "1.0.0",
            "title": title,
            "description": null,
            "publisherName": "Zeron Tests",
            "language": "en",
            "category": null,
            "tags": ["rust"],
            "prerequisites": [],
            "licenseName": "MIT"
        }))
        .unwrap()
    }

    #[test]
    fn create_list_get_round_trip_without_paths() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let created = create(&mut drafts).unwrap();
        let listed = list(&drafts).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].draft_id, created.draft_id);
        let loaded = detail(&drafts, &created.draft_id).unwrap().unwrap();
        assert_eq!(loaded.revision, 0);
        assert_eq!(
            loaded.updated_at_unix_seconds,
            created.updated_at_unix_seconds
        );
        assert!(detail(&drafts, "draft_missing").unwrap().is_none());
    }

    #[test]
    fn metadata_save_returns_next_revision_and_conflicts_report_current() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let saved = update_metadata(&mut drafts, &draft_id, 0, input("First")).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.title, "First");
        assert_eq!(saved.license_name.as_deref(), Some("MIT"));
        let conflict = update_metadata(&mut drafts, &draft_id, 0, input("Stale")).unwrap_err();
        let json = serde_json::to_value(conflict).unwrap();
        assert_eq!(json["code"], "revision_conflict");
        assert_eq!(json["currentRevision"], 1);
        assert!(json.get("field").is_none());
    }

    #[test]
    fn invalid_input_names_the_field_and_unknown_input_is_rejected() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let error = update_metadata(&mut drafts, &draft_id, 0, input("bad\u{0}")).unwrap_err();
        let json = serde_json::to_value(error).unwrap();
        assert_eq!(json["code"], "invalid_input");
        assert_eq!(json["field"], "metadata.title");
        let unknown: Result<DraftMetadataInput, _> = serde_json::from_value(serde_json::json!({
            "courseVersion": "1.0.0", "title": "T", "description": null, "publisherName": "P",
            "language": "en", "category": null, "tags": [], "prerequisites": [],
            "licenseName": null, "courseId": "course_hijack"
        }));
        assert!(unknown.is_err());
    }

    #[test]
    fn structure_edits_round_trip_through_tagged_input() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let edit = |value| serde_json::from_value::<StructureEditInput>(value).unwrap();
        let added = edit_structure(
            &mut drafts,
            &draft_id,
            0,
            edit(serde_json::json!({ "type": "addModule", "title": "Basics" })),
        )
        .unwrap();
        let module_id = added.modules[0].id.clone();
        let with_lesson = edit_structure(
            &mut drafts,
            &draft_id,
            1,
            edit(
                serde_json::json!({ "type": "addLesson", "moduleId": module_id, "title": "Hello" }),
            ),
        )
        .unwrap();
        assert_eq!(with_lesson.revision, 2);
        assert_eq!(with_lesson.modules[0].lessons[0].title, "Hello");
        let missing = edit_structure(
            &mut drafts,
            &draft_id,
            2,
            edit(serde_json::json!({ "type": "removeLesson", "lessonId": "lesson_missing" })),
        )
        .unwrap_err();
        assert_eq!(missing.code, "entity_not_found");
        let out_of_range = edit_structure(
            &mut drafts,
            &draft_id,
            2,
            edit(serde_json::json!({ "type": "moveModule", "moduleId": module_id, "position": 4 })),
        )
        .unwrap_err();
        assert_eq!(out_of_range.code, "invalid_position");
        for rejected in [
            serde_json::json!({ "type": "addModule", "title": "T", "id": "module_chosen" }),
            serde_json::json!({ "type": "deleteEverything" }),
        ] {
            assert!(serde_json::from_value::<StructureEditInput>(rejected).is_err());
        }
    }

    #[test]
    fn attached_source_detail_reports_size_without_its_directory() {
        let directory =
            std::env::temp_dir().join(format!("zeron-admin-attach-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("intro.webm");
        std::fs::write(&source, b"12345").unwrap();
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let edit = |value| serde_json::from_value::<StructureEditInput>(value).unwrap();
        let module_id = edit_structure(
            &mut drafts,
            &draft_id,
            0,
            edit(serde_json::json!({ "type": "addModule", "title": "M" })),
        )
        .unwrap()
        .modules[0]
            .id
            .clone();
        let lesson_id = edit_structure(
            &mut drafts,
            &draft_id,
            1,
            edit(serde_json::json!({ "type": "addLesson", "moduleId": module_id, "title": "L" })),
        )
        .unwrap()
        .modules[0]
            .lessons[0]
            .id
            .clone();
        drafts
            .attach_source(&draft_id, 2, &lesson_id, &source)
            .unwrap();
        let json = serde_json::to_value(detail(&drafts, &draft_id).unwrap().unwrap()).unwrap();
        let resource = &json["modules"][0]["lessons"][0]["resources"][0];
        assert_eq!(resource["fileName"], "intro.webm");
        assert_eq!(resource["byteLength"], 5);
        assert_eq!(resource["kind"], "video");
        assert!(!json.to_string().contains(&*directory.to_string_lossy()));
        let resource_id = resource["id"].as_str().unwrap().to_owned();
        let removed = edit_structure(
            &mut drafts,
            &draft_id,
            3,
            edit(serde_json::json!({ "type": "removeResource", "resourceId": resource_id })),
        )
        .unwrap();
        assert!(removed.modules[0].lessons[0].resources.is_empty());
        assert!(source.is_file());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn media_findings_flatten_per_resource_with_display_severity() {
        let directory =
            std::env::temp_dir().join(format!("zeron-admin-media-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let subtitles = directory.join("captions.srt");
        std::fs::write(&subtitles, b"1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let mut stored = drafts.create().unwrap();
        stored.draft.modules.push(DraftModule {
            id: "module_one".to_owned(),
            title: "Module".to_owned(),
            lessons: vec![DraftLesson {
                id: "lesson_one".to_owned(),
                title: "Lesson".to_owned(),
                estimated_duration_seconds: None,
                resources: vec![DraftResource {
                    id: "subs_one".to_owned(),
                    kind: ResourceKind::Subtitle,
                    source_path: subtitles,
                    package_path: "resources/subs_one/captions.srt".to_owned(),
                    media_type: "application/x-subrip".to_owned(),
                    duration_seconds: Some(42),
                    language: None,
                }],
            }],
        });
        let json = serde_json::to_value(media_findings(&stored.draft)).unwrap();
        assert_eq!(json[0]["resourceId"], "subs_one");
        assert_eq!(json[0]["field"], "modules[0].lessons[0].resources[0]");
        assert_eq!(json[0]["fileName"], "captions.srt");
        assert_eq!(json[0]["code"], "subtitle_format");
        assert_eq!(json[0]["severity"], "advice");
        assert!(!json.to_string().contains(&*directory.to_string_lossy()));
        let detail = serde_json::to_value(DraftDetailDto::from(stored)).unwrap();
        assert_eq!(
            detail["modules"][0]["lessons"][0]["resources"][0]["durationSeconds"],
            42
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// A temporary directory and a draft with one attached PDF, ready to publish.
    fn publishable_draft(name: &str) -> (PathBuf, PublisherDrafts, course_publisher::DraftCourse) {
        let directory =
            std::env::temp_dir().join(format!("zeron-admin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("notes.pdf");
        std::fs::write(&source, b"%PDF-1.7 tiny").unwrap();
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        update_metadata(&mut drafts, &draft_id, 0, input("Publishable")).unwrap();
        let edit = |value| serde_json::from_value::<StructureEditInput>(value).unwrap();
        let module_id = edit_structure(
            &mut drafts,
            &draft_id,
            1,
            edit(serde_json::json!({ "type": "addModule", "title": "M" })),
        )
        .unwrap()
        .modules[0]
            .id
            .clone();
        let lesson_id = edit_structure(
            &mut drafts,
            &draft_id,
            2,
            edit(serde_json::json!({ "type": "addLesson", "moduleId": module_id, "title": "L" })),
        )
        .unwrap()
        .modules[0]
            .lessons[0]
            .id
            .clone();
        drafts
            .attach_source(&draft_id, 3, &lesson_id, &source)
            .unwrap();
        let draft = drafts.draft(&draft_id).unwrap().unwrap().draft;
        (directory, drafts, draft)
    }

    #[test]
    fn publish_job_dto_uses_screaming_stages_and_hides_paths() {
        let (directory, mut drafts, draft) = publishable_draft("publish");
        let jobs = publisher_application::PublishJobs::new(&directory.join("staging"));
        let job = jobs.begin(&draft).unwrap();
        let mut stages = Vec::new();
        let finished = jobs
            .run(
                &job.job_id,
                &draft,
                &DistributionOptions::default(),
                &mut |snapshot| {
                    stages.push(
                        serde_json::to_value(PublishJobDto::from(snapshot)).unwrap()["stage"]
                            .clone(),
                    );
                },
            )
            .unwrap();
        assert_eq!(stages.first().unwrap(), "VALIDATING");
        assert_eq!(stages.last().unwrap(), "READY_TO_PUBLISH");
        let json = serde_json::to_value(PublishJobDto::from(&finished)).unwrap();
        assert_eq!(json["prepared"]["infoHash"].as_str().unwrap().len(), 40);
        assert_eq!(json["prepared"]["fileCount"], 2);
        assert!(json["failure"].is_null());
        assert!(json["releaseId"].is_null());
        assert!(!json.to_string().contains(&*directory.to_string_lossy()));

        let (record, published) = jobs
            .activate(&job.job_id, &mut drafts, &directory.join("releases"))
            .unwrap();
        let release = serde_json::to_value(ReleaseDto::from(record)).unwrap();
        assert_eq!(release["status"], "published");
        assert_eq!(release["courseVersion"], "1.0.0");
        assert_eq!(release["fileCount"], 2);
        assert!(!release.to_string().contains(&*directory.to_string_lossy()));
        let published = serde_json::to_value(PublishJobDto::from(&published)).unwrap();
        assert_eq!(published["stage"], "PUBLISHED");
        assert_eq!(published["releaseId"], release["releaseId"]);
        let repeat = jobs.begin(&draft).unwrap();
        jobs.run(
            &repeat.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        );
        let error = jobs
            .activate(&repeat.job_id, &mut drafts, &directory.join("releases"))
            .map_err(|error| service_error("release_activation_failed", error))
            .unwrap_err();
        assert_eq!(error.code, "version_already_released");
        make_writable(&directory);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn release_details_and_torrent_exports_are_path_free() {
        let (directory, mut drafts, draft) = publishable_draft("export");
        let jobs = publisher_application::PublishJobs::new(&directory.join("staging"));
        let job = jobs.begin(&draft).unwrap();
        jobs.run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        )
        .unwrap();
        let (record, _) = jobs
            .activate(&job.job_id, &mut drafts, &directory.join("releases"))
            .unwrap();
        let release = serde_json::to_value(ReleaseDto::from(record)).unwrap();
        let release_id = release["releaseId"].as_str().unwrap().to_owned();
        let releases_root = directory.join("releases");
        let details = release_torrent(&drafts, &releases_root, &release_id).unwrap();
        let exported = directory.join("exported.torrent");
        write_export(&exported, &details.bytes).unwrap();
        assert_eq!(std::fs::read(&exported).unwrap(), details.bytes);
        assert!(
            !std::fs::metadata(&exported)
                .unwrap()
                .permissions()
                .readonly()
        );
        let details = serde_json::to_value(ReleaseDetailsDto::from(details)).unwrap();
        assert_eq!(details["release"], release);
        assert_eq!(details["files"].as_array().unwrap().len(), 2);
        assert!(
            details["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file["path"] == "course.json")
        );
        assert!(!details.to_string().contains(&*directory.to_string_lossy()));
        let missing = release_torrent(&drafts, &releases_root, "../releases").unwrap_err();
        assert_eq!(missing.code, "release_unavailable");
        make_writable(&directory);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn make_writable(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                make_writable(&path);
            } else {
                let mut permissions = std::fs::metadata(&path).unwrap().permissions();
                #[expect(clippy::permissions_set_readonly_false, reason = "test cleanup")]
                permissions.set_readonly(false);
                std::fs::set_permissions(&path, permissions).unwrap();
            }
        }
    }

    #[test]
    fn deleting_drafts_reports_busy_conflict_and_missing_codes() {
        let (directory, mut drafts, draft) = publishable_draft("delete");
        let jobs = publisher_application::PublishJobs::new(&directory.join("staging"));
        let job = jobs.begin(&draft).unwrap();
        let busy = delete(&mut drafts, &jobs, &draft.draft_id, draft.revision).unwrap_err();
        assert_eq!(busy.code, "draft_busy");
        assert!(jobs.cancel(&job.job_id));
        jobs.run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        );

        let stale = delete(&mut drafts, &jobs, &draft.draft_id, 0).unwrap_err();
        assert_eq!(stale.code, "revision_conflict");
        assert_eq!(stale.current_revision, Some(draft.revision));
        delete(&mut drafts, &jobs, &draft.draft_id, draft.revision).unwrap();
        assert!(list(&drafts).unwrap().is_empty());
        let missing = delete(&mut drafts, &jobs, &draft.draft_id, draft.revision).unwrap_err();
        assert_eq!(missing.code, "draft_not_found");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn archiving_without_an_engine_marks_the_release_archived() {
        let (directory, mut drafts, draft) = publishable_draft("archive");
        let jobs = publisher_application::PublishJobs::new(&directory.join("staging"));
        let job = jobs.begin(&draft).unwrap();
        jobs.run(
            &job.job_id,
            &draft,
            &DistributionOptions::default(),
            &mut |_| {},
        )
        .unwrap();
        let releases_root = directory.join("releases");
        let (record, _) = jobs
            .activate(&job.job_id, &mut drafts, &releases_root)
            .unwrap();
        let release_id = record.release.release_id;

        let archived = serde_json::to_value(archive(&mut drafts, &release_id).unwrap()).unwrap();
        assert_eq!(archived["status"], "archived");
        assert!(
            releases_root
                .join(&release_id)
                .join("release.torrent")
                .is_file()
        );
        let refused = drafts
            .seed_request(&releases_root, &release_id)
            .map_err(|error| service_error("seeding_failed", error))
            .unwrap_err();
        assert_eq!(refused.code, "release_archived");
        let unknown = archive(&mut drafts, "release_0123456789abcdef0123456789abcdef").unwrap_err();
        assert_eq!(unknown.code, "release_archive_failed");
        assert_eq!(archived["files"], "present");

        let wrong = delete_files(&mut drafts, &releases_root, &release_id, "9.9.9").unwrap_err();
        assert_eq!(wrong.code, "invalid_input");
        assert_eq!(wrong.field.as_deref(), Some("confirmation"));
        let deleted = serde_json::to_value(
            delete_files(&mut drafts, &releases_root, &release_id, "1.0.0").unwrap(),
        )
        .unwrap();
        assert_eq!(deleted["status"], "archived");
        assert_eq!(deleted["files"], "deleted");
        assert!(!releases_root.join(&release_id).exists());
        assert!(!deleted.to_string().contains(&*directory.to_string_lossy()));
        make_writable(&directory);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn suggested_torrent_names_are_portable() {
        assert_eq!(
            torrent_file_name("Rust Systems: Part 1", "1.2.0"),
            "rust-systems-part-1-1.2.0.torrent"
        );
        assert_eq!(
            torrent_file_name("../../etc/passwd", "1.0.0"),
            "etc-passwd-1.0.0.torrent"
        );
        assert_eq!(torrent_file_name("Ünïcode ✓", ""), "n-code.torrent");
        assert_eq!(torrent_file_name("///", "2.0.0"), "course-2.0.0.torrent");
        assert!(torrent_file_name(&"x".repeat(500), "1.0.0").len() < 90);
    }

    #[test]
    fn seeding_status_dto_reports_local_metrics_and_failures() {
        let record = publisher_application::ReleaseRecord {
            release: publisher_application::NewRelease {
                release_id: "release_one".to_owned(),
                course_id: "course_one".to_owned(),
                course_version: "1.0.0".to_owned(),
                title: "Course".to_owned(),
                draft_id: "draft_one".to_owned(),
                draft_revision: 3,
                manifest_sha256: "f".repeat(64),
                info_hash: "a".repeat(40),
                magnet: "magnet:?xt=urn:btih:a".to_owned(),
                piece_length: 16_384,
                piece_count: 4,
                file_count: 2,
                total_bytes: 60_000,
            },
            published_at_unix_seconds: 1,
            status: publisher_application::ReleaseStatus::Published,
            files: publisher_application::ReleaseFiles::Present,
        };
        let stopped = serde_json::to_value(SeedingStatusDto::from(ReleaseSeedingStatus {
            release: record.clone(),
            enabled: false,
            transfer: None,
            start_error: Some("the release files differ from their record".to_owned()),
        }))
        .unwrap();
        assert!(stopped["state"].is_null());
        assert_eq!(stopped["totalBytes"], 60_000);
        assert_eq!(stopped["releaseStatus"], "published");
        assert_eq!(
            stopped["failure"],
            "the release files differ from their record"
        );

        let seeding = serde_json::to_value(SeedingStatusDto::from(ReleaseSeedingStatus {
            release: record,
            enabled: true,
            transfer: Some(publisher_application::TransferStats {
                info_hash: torrent_port_hash(),
                state: TransferState::Seeding,
                verified_bytes: 60_000,
                total_bytes: 60_000,
                uploaded_bytes: 120_000,
                upload_bytes_per_second: 2_048,
                downloaded_bytes: 0,
                connected_peers: 3,
                file_verified_bytes: vec![60_000],
                failure: None,
            }),
            start_error: None,
        }))
        .unwrap();
        assert_eq!(seeding["state"], "seeding");
        assert_eq!(seeding["enabled"], true);
        assert_eq!(seeding["uploadedBytes"], 120_000);
        assert_eq!(seeding["connectedPeers"], 3);
        assert!(seeding["failure"].is_null());
        assert!(parse_network_profile("internet").is_some());
        assert!(parse_network_profile("everything").is_none());
        assert_eq!(
            parse_reachability(Some(51413), true).unwrap(),
            PeerReachability::new(Some(51413), true).unwrap()
        );
        let error = parse_reachability(Some(80), false).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(error.field.as_deref(), Some("peerPort"));
    }

    fn torrent_port_hash() -> publisher_application::InfoHash {
        publisher_application::InfoHash::parse(&"a".repeat(40)).unwrap()
    }

    #[test]
    fn validation_issues_are_path_free() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let issues = validate(&drafts, &draft_id).unwrap();
        assert!(issues.iter().any(|issue| issue.code == "empty_course"));
        let error = validate(&drafts, "draft_missing").unwrap_err();
        assert_eq!(error.code, "draft_not_found");
    }

    #[test]
    fn detail_dto_summarizes_resources_without_exposing_source_paths() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let mut stored = drafts.create().unwrap();
        stored.updated_at_unix_seconds = 7;
        stored.draft.modules.push(DraftModule {
            id: "module_one".to_owned(),
            title: "Module".to_owned(),
            lessons: vec![DraftLesson {
                id: "lesson_one".to_owned(),
                title: "Lesson".to_owned(),
                estimated_duration_seconds: None,
                resources: vec![DraftResource {
                    id: "video_one".to_owned(),
                    kind: ResourceKind::Video,
                    source_path: PathBuf::from("/home/publisher/secret/video.mp4"),
                    package_path: "media/video.mp4".to_owned(),
                    media_type: "video/mp4".to_owned(),
                    duration_seconds: None,
                    language: None,
                }],
            }],
        });
        let json = serde_json::to_value(DraftDetailDto::from(stored)).unwrap();
        let resource = &json["modules"][0]["lessons"][0]["resources"][0];
        assert_eq!(resource["id"], "video_one");
        assert_eq!(resource["kind"], "video");
        assert_eq!(resource["fileName"], "video.mp4");
        assert_eq!(resource["mediaType"], "video/mp4");
        assert!(resource["byteLength"].is_null());
        assert_eq!(json["updatedAtUnixSeconds"], 7);
        let text = json.to_string();
        assert!(!text.contains("/home/"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("sourcePath"));
        assert!(!text.contains("packagePath"));
    }
}
