use std::{
    io::Read as _,
    sync::{Arc, Mutex},
};

use course_format::ResourceKind;
use learning_application::{
    Anchor, Availability, CacheError, CancellationFlag, CourseImportReceipt, CourseLibrary,
    CourseSharingStatus, DISK_RESERVE_BYTES, DocumentError, DocumentSource, DownloadScope,
    DrawingFile, DrawingSave, EngineError, ImportError, InfoHash, LibraryError, LocalResource,
    NetworkProfile, Note, NoteChange, PeerReachability, RateLimits, SharingError, TorrentEngine,
    TorrentImportSnapshot, TorrentImportStage, TorrentResource, TorrentSource, TransferState, fits,
    read_local_document, read_torrent_document,
};
use media_gateway::{FileResource, StreamResource};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::{AppState, Router, media::TorrentMedia};

const TORRENT_IMPORT_EVENT: &str = "torrent-import";
/// Course torrents list a handful of files; anything larger is not a Zeron release.
const MAX_TORRENT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MAGNET_BYTES: usize = 8 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadinessDto {
    state: &'static str,
    torrent_import: &'static str,
    sharing: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CourseSummaryDto {
    course_id: String,
    course_version: String,
    title: String,
    publisher_name: String,
    language: String,
    total_resource_bytes: u64,
    imported_at_unix_seconds: u64,
    source: &'static str,
    torrent_info_hash: Option<String>,
    sharing: bool,
    completed_lessons: usize,
    total_lessons: usize,
}

/// A learner note. `body` is the Tiptap document as JSON text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NoteDto {
    note_id: String,
    lesson_id: String,
    anchor: Anchor,
    body: String,
    text: String,
    revision: u64,
    created_at_unix_seconds: u64,
    updated_at_unix_seconds: u64,
}

/// Result of changing an existing note; a conflict overwrites nothing.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(crate) enum NoteChangeDto {
    Saved {
        note: NoteDto,
    },
    #[serde(rename_all = "camelCase")]
    Conflict {
        current_revision: u64,
    },
    NotFound,
}

impl From<Note> for NoteDto {
    fn from(note: Note) -> Self {
        Self {
            note_id: note.note_id,
            lesson_id: note.lesson_id,
            anchor: note.anchor,
            body: note.body_json,
            text: note.text,
            revision: note.revision,
            created_at_unix_seconds: note.created_at,
            updated_at_unix_seconds: note.updated_at,
        }
    }
}

impl From<NoteChange> for NoteChangeDto {
    fn from(change: NoteChange) -> Self {
        match change {
            NoteChange::Saved(note) => Self::Saved { note: note.into() },
            NoteChange::Conflict { current_revision } => Self::Conflict { current_revision },
            NoteChange::NotFound => Self::NotFound,
        }
    }
}

/// A lesson drawing as the window receives and sends it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DrawingDto {
    scene_json: String,
    revision: u64,
    files: Vec<DrawingFileDto>,
}

/// An image in a drawing.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DrawingFileDto {
    id: String,
    mime_type: String,
    data_url: String,
}

/// Result of saving a drawing; a conflict overwrites nothing.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(crate) enum DrawingSaveDto {
    Saved {
        revision: u64,
    },
    #[serde(rename_all = "camelCase")]
    Conflict {
        current_revision: u64,
    },
}

/// Progress of a course and of each of its lessons.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CourseProgressDto {
    completed_lessons: usize,
    total_lessons: usize,
    percent: u8,
    lessons: Vec<LessonProgressDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LessonProgressDto {
    lesson_id: String,
    /// `not_started`, `in_progress`, or `completed`.
    status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportReceiptDto {
    course_id: String,
    course_version: String,
    title: String,
    verified_resource_count: usize,
    total_resource_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CourseDetailDto {
    course_id: String,
    course_version: String,
    title: String,
    description: Option<String>,
    publisher_name: String,
    source: &'static str,
    torrent_info_hash: Option<String>,
    modules: Vec<ModuleDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleDto {
    id: String,
    title: String,
    lessons: Vec<LessonDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LessonDto {
    id: String,
    title: String,
    resources: Vec<ResourceDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceDto {
    id: String,
    kind: &'static str,
    media_type: String,
    byte_length: u64,
    duration_seconds: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaRegistrationDto {
    url: String,
    media_type: String,
    byte_length: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TorrentImportDto {
    job_id: String,
    sequence: u64,
    stage: &'static str,
    info_hash: Option<String>,
    manifest_verified_bytes: Option<u64>,
    manifest_total_bytes: Option<u64>,
    course_id: Option<String>,
    title: Option<String>,
    failure: Option<TorrentImportFailureDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TorrentImportFailureDto {
    code: &'static str,
    message: String,
}

/// Configured and active network profile, with this session's peer ports.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkSettingsDto {
    configured_profile: &'static str,
    active_profile: &'static str,
    torrent_engine: &'static str,
    download_port: Option<u16>,
    sharing_port: Option<u16>,
    /// Stored fixed port for shared courses; `None` means a new port on each start.
    peer_port: Option<u16>,
    /// Stored `UPnP` choice, honored under the Internet profile only.
    port_forwarding: bool,
    /// Whether this session asks the router to forward Zeron's ports.
    active_port_forwarding: bool,
    /// The stored network choice differs from the running engines'.
    restart_required: bool,
    /// Download cap in KiB/s; `None` means unlimited. Applies immediately.
    download_limit_kib: Option<u32>,
    /// Upload cap for shared courses in KiB/s; `None` means unlimited.
    upload_limit_kib: Option<u32>,
}

/// Verified bytes of one torrent lesson on this computer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceAvailabilityDto {
    resource_id: String,
    verified_bytes: u64,
    byte_length: u64,
    /// Whether the learner keeps this lesson offline.
    kept: bool,
    /// `online_only`, `downloading`, `partially_available`, or `available_offline`.
    state: &'static str,
}

/// What a removal freed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovalDto {
    removed: Vec<String>,
    freed_bytes: u64,
    /// Requested lessons left alone because they are kept offline.
    kept: Vec<String>,
}

/// Disk use of streamed lessons and the learner's cap on it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageDto {
    /// Verified bytes of lessons that are not kept offline.
    streamed_bytes: u64,
    cache_limit_bytes: Option<u64>,
    /// Free bytes on the downloads disk, when the platform can tell.
    available_bytes: Option<u64>,
    /// Downloads are paused because the disk is nearly full.
    low_disk: bool,
}

/// What a torrent course keeps offline.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CourseDownloadsDto {
    /// Whether the learner paused this course's transfer.
    paused: bool,
    course: bool,
    modules: Vec<String>,
    lessons: Vec<String>,
}

/// Part of a course to keep offline.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum DownloadScopeInput {
    // A struct variant, so unknown fields are rejected here too.
    Course {},
    Module { id: String },
    Lesson { id: String },
}

/// A torrent course's sharing choice and metrics measured by this computer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CourseSharingDto {
    course_id: String,
    available: bool,
    sharing: bool,
    network_profile: &'static str,
    state: &'static str,
    connected_peers: u32,
    uploaded_bytes: u64,
    upload_bytes_per_second: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: String,
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn readiness(state: State<'_, AppState>) -> ReadinessDto {
    ReadinessDto {
        state: "ready",
        torrent_import: if state.torrents.imports.is_some() {
            "ready"
        } else {
            "unavailable"
        },
        sharing: if state
            .torrents
            .sharing
            .as_ref()
            .is_some_and(|sharing| sharing.available())
        {
            "ready"
        } else {
            "unavailable"
        },
    }
}

#[tauri::command]
pub(crate) async fn list_courses(
    state: State<'_, AppState>,
) -> Result<Vec<CourseSummaryDto>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let library = lock_library(&library)?;
        let progress = |course_id: &str| {
            library
                .course_progress(course_id)
                .ok()
                .flatten()
                .map(|view| view.summary)
                .unwrap_or_default()
        };
        library
            .courses()
            .map(|courses| {
                courses
                    .into_iter()
                    .map(|course| {
                        let summary = progress(&course.course_id);
                        CourseSummaryDto {
                            completed_lessons: summary.completed,
                            total_lessons: summary.total,
                            course_id: course.course_id,
                            course_version: course.course_version,
                            title: course.title,
                            publisher_name: course.publisher_name,
                            language: course.language,
                            total_resource_bytes: course.total_resource_bytes,
                            imported_at_unix_seconds: course.imported_at_unix_seconds,
                            source: source_name(course.torrent_info_hash.as_deref()),
                            torrent_info_hash: course.torrent_info_hash,
                            sharing: course.sharing,
                        }
                    })
                    .collect()
            })
            .map_err(|error| command_error("library_query_failed", error))
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_course_detail(
    course_id: String,
    state: State<'_, AppState>,
) -> Result<Option<CourseDetailDto>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let library = lock_library(&library)?;
        let failed = |error| command_error("course_detail_failed", error);
        let Some(manifest) = library.manifest(&course_id).map_err(failed)? else {
            return Ok(None);
        };
        let info_hash = library.torrent_info_hash(&course_id).map_err(failed)?;
        Ok(Some(CourseDetailDto::new(manifest, info_hash)))
    })
    .await
}

#[tauri::command]
pub(crate) async fn choose_and_import_course(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ImportReceiptDto>, CommandError> {
    let selected = run_blocking(move || Ok(app.dialog().file().blocking_pick_folder())).await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let root = selected.into_path().map_err(|_| CommandError {
        code: "unsupported_folder_location",
        message: "the selected folder is not on a local disk".to_owned(),
    })?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let mut library = lock_library(&library)?;
        library
            .import_local_folder(&root, &CancellationFlag::default())
            .map(ImportReceiptDto::from)
            .map(Some)
            .map_err(local_import_error)
    })
    .await
}

fn local_import_error(error: LibraryError) -> CommandError {
    match error {
        // OS, SQLite, and canonicalization errors can name local directories;
        // they never cross IPC.
        LibraryError::Import(ImportError::Io { operation, .. }) => CommandError {
            code: "course_import_failed",
            message: format!("the course folder could not be read while {operation}"),
        },
        LibraryError::PackageLocation(_) => CommandError {
            code: "course_import_failed",
            message: "the course folder location could not be resolved".to_owned(),
        },
        LibraryError::Storage(_) => CommandError {
            code: "library_unavailable",
            message: "the course was verified but could not be saved to the library".to_owned(),
        },
        // Remaining import failures name only manifest-relative paths, which
        // `course.json` validation keeps relative.
        LibraryError::Import(_) => command_error("course_import_failed", error),
        LibraryError::InvalidPlaybackTime(_) | LibraryError::Note(_) | LibraryError::Drawing(_) => {
            command_error("invalid_input", error)
        }
        LibraryError::NoteTarget(_) => command_error("note_target_invalid", error),
    }
}

#[tauri::command]
pub(crate) async fn choose_torrent_file_and_import(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<TorrentImportDto>, CommandError> {
    let dialog = app.clone();
    let selected = run_blocking(move || {
        Ok(dialog
            .dialog()
            .file()
            .add_filter("Torrent", &["torrent"])
            .blocking_pick_file())
    })
    .await?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| CommandError {
        code: "unsupported_file_location",
        message: error.to_string(),
    })?;
    let bytes = run_blocking(move || read_torrent_file(&path)).await?;
    start_torrent_import(app, &state, TorrentSource::Metainfo(bytes)).map(Some)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn import_magnet(
    app: AppHandle,
    uri: String,
    state: State<'_, AppState>,
) -> Result<TorrentImportDto, CommandError> {
    let uri = validate_magnet(&uri)?;
    start_torrent_import(app, &state, TorrentSource::Magnet(uri))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn get_torrent_import(
    job_id: String,
    state: State<'_, AppState>,
) -> Option<TorrentImportDto> {
    state
        .torrents
        .imports
        .as_ref()?
        .snapshot(&job_id)
        .as_ref()
        .map(TorrentImportDto::from)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn cancel_torrent_import(job_id: String, state: State<'_, AppState>) -> bool {
    state
        .torrents
        .imports
        .as_ref()
        .is_some_and(|imports| imports.cancel(&job_id))
}

fn start_torrent_import(
    app: AppHandle,
    state: &AppState,
    source: TorrentSource,
) -> Result<TorrentImportDto, CommandError> {
    let imports = state.torrents.imports.clone().ok_or_else(|| CommandError {
        code: "torrent_engine_unavailable",
        message: "torrent import is unavailable because the torrent engine could not start"
            .to_owned(),
    })?;
    let started = imports
        .begin()
        .map_err(|error| command_error("torrent_import_busy", error))?;
    let job_id = started.job_id.clone();
    tauri::async_runtime::spawn(async move {
        imports
            .run(&job_id, source, Vec::new(), &mut |snapshot| {
                let _ = app.emit(TORRENT_IMPORT_EVENT, TorrentImportDto::from(snapshot));
            })
            .await;
    });
    Ok(TorrentImportDto::from(&started))
}

fn read_torrent_file(path: &std::path::Path) -> Result<Vec<u8>, CommandError> {
    let unreadable = |_| CommandError {
        code: "torrent_file_unreadable",
        message: "the selected torrent file could not be read".to_owned(),
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_TORRENT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > MAX_TORRENT_FILE_BYTES {
        return Err(CommandError {
            code: "torrent_file_too_large",
            message: "the selected file is too large to be a course torrent".to_owned(),
        });
    }
    Ok(bytes)
}

fn validate_magnet(uri: &str) -> Result<String, CommandError> {
    let uri = uri.trim();
    if uri.len() > MAX_MAGNET_BYTES || !uri.to_ascii_lowercase().starts_with("magnet:?") {
        return Err(CommandError {
            code: "invalid_magnet",
            message: "paste a magnet link that starts with magnet:?".to_owned(),
        });
    }
    Ok(uri.to_owned())
}

#[tauri::command]
pub(crate) async fn get_course_sharing(
    course_id: String,
    state: State<'_, AppState>,
) -> Result<Option<CourseSharingDto>, CommandError> {
    let Some(sharing) = state.torrents.sharing.clone() else {
        return Ok(None);
    };
    let profile = state.torrents.profile;
    run_blocking(move || {
        let available = sharing.available();
        sharing
            .statuses()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .find(|status| status.course_id == course_id)
                    .map(|status| CourseSharingDto::new(status, available, profile))
            })
            .map_err(|error| command_error("library_query_failed", error))
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_course_sharing(
    course_id: String,
    sharing: bool,
    state: State<'_, AppState>,
) -> Result<CourseSharingDto, CommandError> {
    let service = state.torrents.sharing.clone().ok_or_else(|| CommandError {
        code: "sharing_unavailable",
        message: "sharing is unavailable because the torrent engine could not start".to_owned(),
    })?;
    service
        .set(&course_id, sharing)
        .await
        .map_err(sharing_error)?;
    get_course_sharing(course_id, state)
        .await?
        .ok_or_else(|| CommandError {
            code: "not_torrent_course",
            message: "only courses imported from a torrent can be shared".to_owned(),
        })
}

#[tauri::command]
pub(crate) async fn get_network_settings(
    state: State<'_, AppState>,
) -> Result<NetworkSettingsDto, CommandError> {
    let library = Arc::clone(&state.library);
    let torrents = &state.torrents;
    let (active, available, download_port, sharing_port) = (
        (torrents.profile, torrents.reachability),
        torrents.imports.is_some(),
        torrents.download_port,
        torrents.sharing_port,
    );
    let (configured, reachability, limits) = run_blocking(move || {
        let library = lock_library(&library)?;
        let failed = |error| command_error("settings_failed", error);
        Ok((
            library.network_profile().map_err(failed)?,
            library.peer_reachability().map_err(failed)?,
            library.rate_limits().map_err(failed)?,
        ))
    })
    .await?;
    let (download_limit_kib, upload_limit_kib) = limits.as_kib_per_second();
    Ok(NetworkSettingsDto {
        configured_profile: configured.name(),
        active_profile: active.0.name(),
        torrent_engine: if available { "ready" } else { "unavailable" },
        download_port,
        sharing_port,
        peer_port: reachability.port,
        port_forwarding: reachability.port_forwarding,
        // Any running engine asks the router to forward its port.
        active_port_forwarding: active.1.port_forwarding
            && (download_port.is_some() || sharing_port.is_some()),
        restart_required: (configured, reachability.for_profile(configured)) != active,
        download_limit_kib,
        upload_limit_kib,
    })
}

/// Store speed caps in KiB/s (`None` or 0 for unlimited) and apply them to
/// the running engines at once.
#[tauri::command]
pub(crate) async fn set_rate_limits(
    download_kib: Option<u32>,
    upload_kib: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let limits = parse_rate_limits(download_kib, upload_kib)?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .set_rate_limits(limits)
            .map_err(|error| command_error("settings_failed", error))
    })
    .await?;
    if let Some(router) = state.torrents.router.clone() {
        router
            .set_rate_limits(limits)
            .map_err(|error| command_error("settings_failed", error))?;
    }
    Ok(())
}

fn parse_rate_limits(
    download_kib: Option<u32>,
    upload_kib: Option<u32>,
) -> Result<RateLimits, CommandError> {
    RateLimits::from_kib_per_second(download_kib, upload_kib).ok_or_else(|| CommandError {
        code: "invalid_input",
        message: format!(
            "choose at least {} KiB/s, or leave the limit empty for no limit",
            RateLimits::MIN_BYTES_PER_SECOND / 1024
        ),
    })
}

/// Store the profile used the next time the engines start.
#[tauri::command]
pub(crate) async fn set_network_profile(
    profile: String,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let profile = NetworkProfile::from_name(&profile).ok_or_else(|| CommandError {
        code: "invalid_input",
        message: "unknown network profile".to_owned(),
    })?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .set_network_profile(profile)
            .map_err(|error| command_error("settings_failed", error))
    })
    .await
}

/// Store the shared-course port and `UPnP` choice used the next time the engines start.
#[tauri::command]
pub(crate) async fn set_peer_reachability(
    peer_port: Option<u16>,
    port_forwarding: bool,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let reachability = parse_reachability(peer_port, port_forwarding)?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .set_peer_reachability(reachability)
            .map_err(|error| command_error("settings_failed", error))
    })
    .await
}

fn parse_reachability(
    peer_port: Option<u16>,
    port_forwarding: bool,
) -> Result<PeerReachability, CommandError> {
    PeerReachability::new(peer_port, port_forwarding).ok_or_else(|| CommandError {
        code: "invalid_input",
        message: format!(
            "choose a port from {} to 65535",
            PeerReachability::MIN_FIXED_PORT
        ),
    })
}

enum Media {
    Local(LocalResource),
    Torrent(TorrentResource),
}

#[tauri::command]
pub(crate) async fn register_media(
    course_id: String,
    resource_id: String,
    state: State<'_, AppState>,
) -> Result<MediaRegistrationDto, CommandError> {
    let library = Arc::clone(&state.library);
    let playing = course_id.clone();
    let media = run_blocking(move || {
        let mut library = lock_library(&library)?;
        // Opening a lesson starts it; a failure to record that never blocks playback.
        let _ = library.record_resource_opened(&course_id, &resource_id);
        let failed = |error| command_error("media_lookup_failed", error);
        let not_found = || CommandError {
            code: "media_not_found",
            message: "the selected media resource is unavailable".to_owned(),
        };
        if library
            .torrent_info_hash(&course_id)
            .map_err(failed)?
            .is_some()
        {
            return library
                .torrent_resources(&course_id)
                .map_err(failed)?
                .into_iter()
                .find(|resource| resource.resource_id == resource_id)
                .map(Media::Torrent)
                .ok_or_else(not_found);
        }
        library
            .local_resource(&course_id, &resource_id)
            .map_err(failed)?
            .map(Media::Local)
            .ok_or_else(not_found)
    })
    .await?;
    let registration = match media {
        Media::Local(resource) => {
            state
                .media_gateway
                .register(FileResource {
                    path: resource.path,
                    media_type: resource.media_type,
                    byte_length: resource.byte_length,
                })
                .await
        }
        Media::Torrent(resource) => {
            let router = state.torrents.router.clone().ok_or_else(|| CommandError {
                code: "torrent_engine_unavailable",
                message:
                    "torrent lessons are unavailable because the torrent engine could not start"
                        .to_owned(),
            })?;
            let info_hash = InfoHash::parse(&resource.info_hash)
                .map_err(|error| command_error("media_lookup_failed", error))?;
            state.media_gateway.register_stream(StreamResource {
                source: Arc::new(TorrentMedia {
                    router,
                    info_hash,
                    file_index: resource.file_index,
                }),
                media_type: resource.media_type,
                byte_length: resource.byte_length,
            })
        }
    }
    .map_err(|error| command_error("media_registration_failed", error))?;
    // Keep the streamed-lesson cache within its cap, sparing the course being
    // played so its transfer is not restarted under the player.
    if let Some(cache) = state.torrents.cache.clone() {
        tauri::async_runtime::spawn(async move {
            let _ = cache.enforce_limit(Some(&playing)).await;
        });
    }
    // Only one lesson plays at a time: revoking the previous one stops its pending reads.
    if let Ok(mut current) = state.current_media.lock()
        && let Some(previous) = current.replace(registration.url.clone())
    {
        state.media_gateway.revoke(&previous);
    }
    Ok(MediaRegistrationDto {
        url: registration.url,
        media_type: registration.media_type,
        byte_length: registration.byte_length,
    })
}

/// Remember where the learner stopped in a lesson. Returns `false` for an
/// unknown course or resource.
#[tauri::command]
pub(crate) async fn save_playback_position(
    course_id: String,
    resource_id: String,
    position_seconds: f64,
    duration_seconds: f64,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        save_position(
            &mut *lock_library(&library)?,
            &course_id,
            &resource_id,
            position_seconds,
            duration_seconds,
        )
    })
    .await
}

/// Where the learner stopped in a lesson, in seconds.
#[tauri::command]
pub(crate) async fn get_playback_position(
    course_id: String,
    resource_id: String,
    state: State<'_, AppState>,
) -> Result<Option<f64>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .playback_position(&course_id, &resource_id)
            .map_err(|error| command_error("library_query_failed", error))
    })
    .await
}

fn save_position(
    library: &mut CourseLibrary,
    course_id: &str,
    resource_id: &str,
    position_seconds: f64,
    duration_seconds: f64,
) -> Result<bool, CommandError> {
    library
        .save_playback_position(course_id, resource_id, position_seconds, duration_seconds)
        .map_err(|error| match error {
            LibraryError::InvalidPlaybackTime(_) => command_error("invalid_input", error),
            // Storage errors can name the database location; keep them in Rust.
            _ => CommandError {
                code: "library_write_failed",
                message: "the playback position could not be saved".to_owned(),
            },
        })
}

/// How much of each torrent lesson this computer has verified. Empty for local
/// courses, whose resources are verified at import.
#[tauri::command]
pub(crate) async fn get_course_availability(
    course_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ResourceAvailabilityDto>, CommandError> {
    let Some(router) = state.torrents.router.clone() else {
        return Ok(Vec::new());
    };
    let library = Arc::clone(&state.library);
    run_blocking(move || availability(&*lock_library(&library)?, &*router, &course_id)).await
}

/// Which parts of a torrent course the learner keeps offline.
#[tauri::command]
pub(crate) async fn get_course_downloads(
    course_id: String,
    state: State<'_, AppState>,
) -> Result<CourseDownloadsDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || downloads(&*lock_library(&library)?, &course_id)).await
}

/// Keep part of a torrent course offline, or stop keeping it, and update the
/// running transfer's selection. Verified bytes are never deleted here.
#[tauri::command]
pub(crate) async fn set_course_download(
    course_id: String,
    scope: DownloadScopeInput,
    keep: bool,
    state: State<'_, AppState>,
) -> Result<CourseDownloadsDto, CommandError> {
    let library = Arc::clone(&state.library);
    let router = state.torrents.router.clone();
    let disk = state.torrents.disk.clone();
    let (selection, info_hash, kept) = run_blocking(move || {
        let mut library = lock_library(&library)?;
        let scope = match scope {
            DownloadScopeInput::Course {} => DownloadScope::Course,
            DownloadScopeInput::Module { id } => DownloadScope::Module(id),
            DownloadScopeInput::Lesson { id } => DownloadScope::Lesson(id),
        };
        let failed = |error| command_error("library_query_failed", error);
        let already_kept = library
            .downloads(&course_id)
            .map_err(failed)?
            .contains(&scope);
        if !library
            .set_download(&course_id, &scope, keep)
            .map_err(failed)?
        {
            return Err(CommandError {
                code: "download_target_not_found",
                message:
                    "only lessons, modules, and courses imported from a torrent can be kept offline"
                        .to_owned(),
            });
        }
        if keep && !already_kept {
            let needed = remaining_kept_bytes(&library, router.as_deref(), &course_id)?;
            let available = disk.as_ref().and_then(|disk| disk.measure());
            if !fits(available, needed) {
                library
                    .set_download(&course_id, &scope, false)
                    .map_err(failed)?;
                return Err(insufficient_space(needed, available));
            }
        }
        Ok((
            library.download_selection(&course_id).map_err(failed)?,
            library.torrent_info_hash(&course_id).map_err(failed)?,
            downloads(&library, &course_id)?,
        ))
    })
    .await?;
    if let (Some(router), Some(selection), Some(info_hash)) =
        (state.torrents.router.clone(), selection, info_hash)
    {
        let info_hash = InfoHash::parse(&info_hash)
            .map_err(|error| command_error("library_query_failed", error))?;
        // An inactive course picks the selection up when it is restored.
        match router.select_files(&info_hash, &selection).await {
            Ok(()) | Err(EngineError::NotActive) => {}
            Err(error) => return Err(command_error("download_selection_failed", error)),
        }
    }
    Ok(kept)
}

/// Remove downloaded lessons that are not kept offline: the given resources,
/// or every lesson of the course when `resourceIds` is omitted.
#[tauri::command]
pub(crate) async fn remove_downloads(
    course_id: String,
    resource_ids: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<RemovalDto, CommandError> {
    let cache = state.torrents.cache.clone().ok_or_else(cache_unavailable)?;
    let report = cache
        .remove(&course_id, resource_ids.as_deref())
        .await
        .map_err(cache_error)?;
    Ok(RemovalDto {
        removed: report.removed,
        freed_bytes: report.freed_bytes,
        kept: report.kept,
    })
}

/// How much space streamed lessons take, and the stored cap.
#[tauri::command]
pub(crate) async fn get_storage(state: State<'_, AppState>) -> Result<StorageDto, CommandError> {
    let cache = state.torrents.cache.clone().ok_or_else(cache_unavailable)?;
    let disk = state.torrents.disk.clone();
    run_blocking(move || {
        Ok(StorageDto {
            streamed_bytes: cache.streamed_bytes().map_err(cache_error)?,
            cache_limit_bytes: cache.limit().map_err(cache_error)?,
            available_bytes: disk.as_ref().and_then(|disk| disk.measure()),
            low_disk: disk.is_some_and(|disk| disk.state().low),
        })
    })
    .await
}

/// Store the cap on streamed lessons (`None` for no cap) and apply it now.
#[tauri::command]
pub(crate) async fn set_cache_limit(
    limit_bytes: Option<u64>,
    state: State<'_, AppState>,
) -> Result<RemovalDto, CommandError> {
    let limit_bytes = parse_cache_limit(limit_bytes)?;
    let cache = state.torrents.cache.clone().ok_or_else(cache_unavailable)?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .set_cache_limit(limit_bytes)
            .map_err(|error| command_error("settings_failed", error))
    })
    .await?;
    let report = cache.enforce_limit(None).await.map_err(cache_error)?;
    Ok(RemovalDto {
        removed: report.removed,
        freed_bytes: report.freed_bytes,
        kept: report.kept,
    })
}

const MIN_CACHE_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

/// A cap of at least 256 MiB, or `None` (also for 0) for no cap.
fn parse_cache_limit(limit_bytes: Option<u64>) -> Result<Option<u64>, CommandError> {
    match limit_bytes {
        None | Some(0) => Ok(None),
        Some(bytes) if bytes >= MIN_CACHE_LIMIT_BYTES => Ok(Some(bytes)),
        Some(_) => Err(CommandError {
            code: "invalid_input",
            message: "choose at least 256 MB, or no limit".to_owned(),
        }),
    }
}

fn cache_unavailable() -> CommandError {
    CommandError {
        code: "torrent_engine_unavailable",
        message: "downloads cannot be managed because the torrent engine could not start"
            .to_owned(),
    }
}

fn cache_error(error: CacheError) -> CommandError {
    match error {
        CacheError::NotTorrentCourse => command_error("not_torrent_course", error),
        CacheError::Engine(_) => command_error("engine_failed", error),
        CacheError::Io(_) => command_error("removal_failed", error),
        // Library errors can name the database location; keep them in Rust.
        CacheError::Library(_) => CommandError {
            code: "library_query_failed",
            message: "the library could not be read".to_owned(),
        },
    }
}

/// The verified bytes of a PDF resource, as a raw binary response. Paths
/// never cross IPC; local files are re-hashed against the manifest.
#[tauri::command]
pub(crate) async fn read_document(
    course_id: String,
    resource_id: String,
    state: State<'_, AppState>,
) -> Result<tauri::ipc::Response, CommandError> {
    let library = Arc::clone(&state.library);
    let source = run_blocking(move || {
        lock_library(&library)?
            .document_source(&course_id, &resource_id)
            .map_err(|error| document_error(DocumentError::Library(error)))
    })
    .await?
    .ok_or_else(|| document_error(DocumentError::NotADocument))?;
    let bytes = match source {
        DocumentSource::Local {
            path,
            sha256,
            byte_length,
        } => {
            run_blocking(move || {
                read_local_document(&path, &sha256, byte_length).map_err(document_error)
            })
            .await?
        }
        DocumentSource::Torrent {
            info_hash,
            file_index,
            byte_length,
        } => {
            let router = state
                .torrents
                .router
                .clone()
                .ok_or_else(|| document_error(DocumentError::Unavailable))?;
            read_torrent_document(&*router, &info_hash, file_index, byte_length)
                .await
                .map_err(document_error)?
        }
    };
    Ok(tauri::ipc::Response::new(bytes))
}

fn document_error(error: DocumentError) -> CommandError {
    let code = match error {
        DocumentError::NotADocument => "not_a_document",
        DocumentError::TooLarge => "document_too_large",
        DocumentError::Changed => "document_changed",
        DocumentError::Unavailable => "document_unavailable",
        // Library errors can name the database location; keep them in Rust.
        DocumentError::Library(_) => {
            return CommandError {
                code: "library_query_failed",
                message: "the library could not be read".to_owned(),
            };
        }
    };
    command_error(code, error)
}

/// Notes of a course, or of one lesson, oldest first.
#[tauri::command]
pub(crate) async fn list_notes(
    course_id: String,
    lesson_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<NoteDto>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        Ok(lock_library(&library)?
            .notes(&course_id, lesson_id.as_deref())
            .map_err(note_error)?
            .into_iter()
            .map(NoteDto::from)
            .collect())
    })
    .await
}

/// Create a note in a lesson at `anchor`.
#[tauri::command]
pub(crate) async fn create_note(
    course_id: String,
    lesson_id: String,
    anchor: Anchor,
    body: String,
    state: State<'_, AppState>,
) -> Result<NoteDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .create_note(&course_id, &lesson_id, &anchor, &body)
            .map(NoteDto::from)
            .map_err(note_error)
    })
    .await
}

/// Replace a note if it is still at `expectedRevision`.
#[tauri::command]
pub(crate) async fn update_note(
    note_id: String,
    expected_revision: u64,
    anchor: Anchor,
    body: String,
    state: State<'_, AppState>,
) -> Result<NoteChangeDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .update_note(&note_id, expected_revision, &anchor, &body)
            .map(NoteChangeDto::from)
            .map_err(note_error)
    })
    .await
}

/// Delete a note if it is still at `expectedRevision`.
#[tauri::command]
pub(crate) async fn delete_note(
    note_id: String,
    expected_revision: u64,
    state: State<'_, AppState>,
) -> Result<NoteChangeDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        lock_library(&library)?
            .delete_note(&note_id, expected_revision)
            .map(NoteChangeDto::from)
            .map_err(note_error)
    })
    .await
}

fn note_error(error: LibraryError) -> CommandError {
    match error {
        LibraryError::Note(_) | LibraryError::Drawing(_) => command_error("invalid_input", error),
        LibraryError::NoteTarget(_) => command_error("note_target_invalid", error),
        // Storage errors can name the database location; keep them in Rust.
        _ => CommandError {
            code: "library_write_failed",
            message: "the note could not be saved".to_owned(),
        },
    }
}

/// The learner's drawing for a lesson, or `null`.
#[tauri::command]
pub(crate) async fn get_drawing(
    course_id: String,
    lesson_id: String,
    state: State<'_, AppState>,
) -> Result<Option<DrawingDto>, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let drawing = lock_library(&library)?
            .drawing(&course_id, &lesson_id)
            .map_err(note_error)?;
        Ok(drawing.map(|drawing| DrawingDto {
            scene_json: drawing.scene_json,
            revision: drawing.revision,
            files: drawing
                .files
                .into_iter()
                .map(|file| DrawingFileDto {
                    id: file.id,
                    mime_type: file.mime_type,
                    data_url: file.data_url,
                })
                .collect(),
        }))
    })
    .await
}

/// Save a lesson's drawing from `expectedRevision` (`null` for a new one).
/// `files` may repeat stored images; only new ones are kept.
#[tauri::command]
pub(crate) async fn save_drawing(
    course_id: String,
    lesson_id: String,
    expected_revision: Option<u64>,
    scene_json: String,
    files: Vec<DrawingFileDto>,
    state: State<'_, AppState>,
) -> Result<DrawingSaveDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let files: Vec<DrawingFile> = files
            .into_iter()
            .map(|file| DrawingFile {
                id: file.id,
                mime_type: file.mime_type,
                data_url: file.data_url,
            })
            .collect();
        let saved = lock_library(&library)?
            .save_drawing(
                &course_id,
                &lesson_id,
                expected_revision,
                &scene_json,
                &files,
            )
            .map_err(note_error)?;
        Ok(match saved {
            DrawingSave::Saved { revision } => DrawingSaveDto::Saved { revision },
            DrawingSave::Conflict { current_revision } => {
                DrawingSaveDto::Conflict { current_revision }
            }
        })
    })
    .await
}

/// Progress of a course and its lessons.
#[tauri::command]
pub(crate) async fn get_course_progress(
    course_id: String,
    state: State<'_, AppState>,
) -> Result<CourseProgressDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || progress(&*lock_library(&library)?, &course_id)).await
}

/// Mark a lesson completed or not, and return the course's progress.
#[tauri::command]
pub(crate) async fn set_lesson_completed(
    course_id: String,
    lesson_id: String,
    completed: bool,
    state: State<'_, AppState>,
) -> Result<CourseProgressDto, CommandError> {
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let mut library = lock_library(&library)?;
        if !library
            .set_lesson_completed(&course_id, &lesson_id, completed)
            .map_err(|error| command_error("library_write_failed", error))?
        {
            return Err(CommandError {
                code: "lesson_not_found",
                message: "the lesson is not part of this course".to_owned(),
            });
        }
        progress(&library, &course_id)
    })
    .await
}

fn progress(library: &CourseLibrary, course_id: &str) -> Result<CourseProgressDto, CommandError> {
    let view = library
        .course_progress(course_id)
        .map_err(|error| command_error("library_query_failed", error))?
        .ok_or_else(|| CommandError {
            code: "course_not_found",
            message: "the course is not in the library".to_owned(),
        })?;
    Ok(CourseProgressDto {
        completed_lessons: view.summary.completed,
        total_lessons: view.summary.total,
        percent: view.summary.percent(),
        lessons: view
            .lessons
            .into_iter()
            .map(|(lesson_id, status)| LessonProgressDto {
                lesson_id,
                status: status.as_str(),
            })
            .collect(),
    })
}

/// Pause or resume a torrent course's transfer. Verified lessons keep
/// playing while paused; the choice survives a restart.
#[tauri::command]
pub(crate) async fn set_course_paused(
    course_id: String,
    paused: bool,
    state: State<'_, AppState>,
) -> Result<CourseDownloadsDto, CommandError> {
    let library = Arc::clone(&state.library);
    let lookup = course_id.clone();
    let info_hash = run_blocking(move || {
        lock_library(&library)?
            .torrent_info_hash(&lookup)
            .map_err(|error| command_error("library_query_failed", error))
    })
    .await?
    .ok_or_else(|| CommandError {
        code: "not_torrent_course",
        message: "only courses imported from a torrent can be paused".to_owned(),
    })?;
    if let Some(router) = state.torrents.router.clone() {
        let info_hash = InfoHash::parse(&info_hash)
            .map_err(|error| command_error("library_query_failed", error))?;
        let applied = if paused {
            router.pause(&info_hash).await
        } else {
            router.resume(&info_hash).await
        };
        // An inactive course picks the choice up when it is restored.
        match applied {
            Ok(()) | Err(EngineError::NotActive) => {}
            Err(error) => return Err(command_error("pause_failed", error)),
        }
    }
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let mut library = lock_library(&library)?;
        library
            .set_course_paused(&course_id, paused)
            .map_err(|error| command_error("library_write_failed", error))?;
        downloads(&library, &course_id)
    })
    .await
}

/// Bytes still to download for every kept lesson of a course.
fn remaining_kept_bytes(
    library: &CourseLibrary,
    engine: Option<&Router>,
    course_id: &str,
) -> Result<u64, CommandError> {
    let failed = |error| command_error("library_query_failed", error);
    let kept = library.committed_resources(course_id).map_err(failed)?;
    let resources = library.torrent_resources(course_id).map_err(failed)?;
    let verified = resources
        .first()
        .and_then(|resource| InfoHash::parse(&resource.info_hash).ok())
        .and_then(|info_hash| engine.and_then(|engine| engine.transfer(&info_hash)))
        .map(|stats| stats.file_verified_bytes)
        .unwrap_or_default();
    Ok(resources
        .iter()
        .filter(|resource| kept.contains(&resource.resource_id))
        .map(|resource| {
            let done = verified.get(resource.file_index).copied().unwrap_or(0);
            resource.byte_length.saturating_sub(done)
        })
        .sum())
}

/// Sizes in the message are rounded to tenths of a gigabyte.
fn insufficient_space(needed: u64, available: Option<u64>) -> CommandError {
    let gib = |bytes: u64| {
        let tenths = (bytes.saturating_mul(10).saturating_add(1 << 29)) / (1 << 30);
        format!("{}.{} GB", tenths / 10, tenths % 10)
    };
    CommandError {
        code: "insufficient_space",
        message: format!(
            "keeping this offline needs about {} more, but only {} is free and Zeron leaves {} free",
            gib(needed),
            gib(available.unwrap_or(0)),
            gib(DISK_RESERVE_BYTES)
        ),
    }
}

fn downloads(library: &CourseLibrary, course_id: &str) -> Result<CourseDownloadsDto, CommandError> {
    let failed = |error| command_error("library_query_failed", error);
    let scopes = library.downloads(course_id).map_err(failed)?;
    let mut dto = CourseDownloadsDto {
        paused: library.course_paused(course_id).map_err(failed)?,
        ..CourseDownloadsDto::default()
    };
    for scope in scopes {
        match scope {
            DownloadScope::Course => dto.course = true,
            DownloadScope::Module(id) => dto.modules.push(id),
            DownloadScope::Lesson(id) => dto.lessons.push(id),
        }
    }
    Ok(dto)
}

fn availability<E: TorrentEngine>(
    library: &CourseLibrary,
    engine: &E,
    course_id: &str,
) -> Result<Vec<ResourceAvailabilityDto>, CommandError> {
    let failed = |error| command_error("library_query_failed", error);
    let resources = library.torrent_resources(course_id).map_err(failed)?;
    let kept = library.committed_resources(course_id).map_err(failed)?;
    let transfer = resources
        .first()
        .and_then(|resource| InfoHash::parse(&resource.info_hash).ok())
        .and_then(|info_hash| engine.transfer(&info_hash));
    Ok(resources
        .into_iter()
        .map(|resource| {
            let verified_bytes = transfer
                .as_ref()
                .and_then(|transfer| {
                    transfer
                        .file_verified_bytes
                        .get(resource.file_index)
                        .copied()
                })
                .unwrap_or(0);
            let kept = kept.contains(&resource.resource_id);
            ResourceAvailabilityDto {
                state: Availability::of(kept, verified_bytes, resource.byte_length).as_str(),
                kept,
                verified_bytes,
                byte_length: resource.byte_length,
                resource_id: resource.resource_id,
            }
        })
        .collect())
}

impl From<CourseImportReceipt> for ImportReceiptDto {
    fn from(receipt: CourseImportReceipt) -> Self {
        Self {
            course_id: receipt.course_id,
            course_version: receipt.course_version,
            title: receipt.title,
            verified_resource_count: receipt.verified_resource_count,
            total_resource_bytes: receipt.total_resource_bytes,
        }
    }
}

impl CourseDetailDto {
    fn new(manifest: course_format::CourseManifest, torrent_info_hash: Option<String>) -> Self {
        Self {
            course_id: manifest.course_id,
            course_version: manifest.version,
            title: manifest.title,
            description: manifest.description,
            publisher_name: manifest.publisher.name,
            source: source_name(torrent_info_hash.as_deref()),
            torrent_info_hash,
            modules: manifest
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
                            resources: lesson
                                .resources
                                .into_iter()
                                .map(|resource| ResourceDto {
                                    id: resource.id,
                                    kind: resource_kind(resource.kind),
                                    media_type: resource.media_type,
                                    byte_length: resource.byte_length,
                                    duration_seconds: resource.duration_seconds,
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<&TorrentImportSnapshot> for TorrentImportDto {
    fn from(snapshot: &TorrentImportSnapshot) -> Self {
        Self {
            job_id: snapshot.job_id.clone(),
            sequence: snapshot.sequence,
            stage: torrent_stage(snapshot.stage),
            info_hash: snapshot.info_hash.clone(),
            manifest_verified_bytes: snapshot.manifest_bytes.map(|(verified, _)| verified),
            manifest_total_bytes: snapshot.manifest_bytes.map(|(_, total)| total),
            course_id: snapshot.course_id.clone(),
            title: snapshot.title.clone(),
            failure: snapshot
                .failure
                .as_ref()
                .map(|failure| TorrentImportFailureDto {
                    code: failure.code,
                    message: failure.message.clone(),
                }),
        }
    }
}

fn sharing_error(error: SharingError) -> CommandError {
    match error {
        SharingError::NotTorrentCourse => command_error("not_torrent_course", error),
        SharingError::Unavailable => command_error("sharing_unavailable", error),
        // Engine details can name local directories; they never cross IPC.
        SharingError::Engine(_) => CommandError {
            code: "sharing_failed",
            message: "the course could not be moved between torrent engines; nothing changed"
                .to_owned(),
        },
        SharingError::Library(_) => command_error("library_unavailable", error),
    }
}

impl CourseSharingDto {
    fn new(status: CourseSharingStatus, available: bool, profile: NetworkProfile) -> Self {
        let transfer = status.transfer.as_ref();
        Self {
            course_id: status.course_id,
            available,
            sharing: status.sharing,
            network_profile: profile.name(),
            state: transfer.map_or("inactive", |transfer| match transfer.state {
                TransferState::Checking => "checking",
                TransferState::Downloading => "downloading",
                TransferState::Seeding => "ready",
                TransferState::Failed => "failed",
                TransferState::Paused => "paused",
            }),
            connected_peers: transfer.map_or(0, |transfer| transfer.connected_peers),
            uploaded_bytes: transfer.map_or(0, |transfer| transfer.uploaded_bytes),
            upload_bytes_per_second: transfer
                .map_or(0, |transfer| transfer.upload_bytes_per_second),
        }
    }
}

const fn torrent_stage(stage: TorrentImportStage) -> &'static str {
    match stage {
        TorrentImportStage::Queued => "queued",
        TorrentImportStage::ResolvingMetadata => "resolving_metadata",
        TorrentImportStage::FetchingManifest => "fetching_manifest",
        TorrentImportStage::Validating => "validating",
        TorrentImportStage::Imported => "imported",
        TorrentImportStage::Failed => "failed",
        TorrentImportStage::Cancelled => "cancelled",
    }
}

const fn source_name(torrent_info_hash: Option<&str>) -> &'static str {
    if torrent_info_hash.is_some() {
        "torrent"
    } else {
        "local"
    }
}

fn resource_kind(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Video => "video",
        ResourceKind::Audio => "audio",
        ResourceKind::Pdf => "pdf",
        ResourceKind::Markdown => "markdown",
        ResourceKind::Text => "text",
        ResourceKind::Subtitle => "subtitle",
        ResourceKind::Transcript => "transcript",
        ResourceKind::Image => "image",
        ResourceKind::SourceArchive => "source_archive",
        ResourceKind::Other => "other",
    }
}

async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError {
            code: "worker_failed",
            message: error.to_string(),
        })?
}

fn lock_library(
    library: &Arc<Mutex<CourseLibrary>>,
) -> Result<std::sync::MutexGuard<'_, CourseLibrary>, CommandError> {
    library.lock().map_err(|_| CommandError {
        code: "library_unavailable",
        message: "the course library worker is unavailable".to_owned(),
    })
}

fn command_error(code: &'static str, error: impl std::fmt::Display) -> CommandError {
    CommandError {
        code,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use learning_application::TorrentImportFailure;

    use super::*;

    #[test]
    fn playback_positions_validate_input_and_ignore_unknown_lessons() {
        let mut library =
            CourseLibrary::open_in_memory(learning_application::ImportLimits::default()).unwrap();
        let invalid =
            save_position(&mut library, "course", "resource", f64::NAN, 10.0).unwrap_err();
        assert_eq!(invalid.code, "invalid_input");
        assert!(!save_position(&mut library, "course", "resource", 1.0, 10.0).unwrap());
    }

    #[test]
    fn download_scopes_parse_strictly_and_non_torrent_courses_keep_nothing() {
        let parse = |value| serde_json::from_value::<DownloadScopeInput>(value);
        assert!(matches!(
            parse(serde_json::json!({ "kind": "course" })).unwrap(),
            DownloadScopeInput::Course {}
        ));
        assert!(matches!(
            parse(serde_json::json!({ "kind": "lesson", "id": "lesson_1" })).unwrap(),
            DownloadScopeInput::Lesson { id } if id == "lesson_1"
        ));
        assert!(parse(serde_json::json!({ "kind": "module" })).is_err());
        assert!(parse(serde_json::json!({ "kind": "file", "id": "x" })).is_err());
        assert!(parse(serde_json::json!({ "kind": "course", "path": "/etc" })).is_err());

        let mut library =
            CourseLibrary::open_in_memory(learning_application::ImportLimits::default()).unwrap();
        assert!(
            !library
                .set_download("course", &DownloadScope::Course, true)
                .unwrap()
        );
        let json = serde_json::to_value(downloads(&library, "course").unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "paused": false, "course": false, "modules": [], "lessons": [] })
        );
    }

    #[test]
    fn cache_limits_are_validated_and_cache_errors_name_no_paths() {
        assert_eq!(parse_cache_limit(None).unwrap(), None);
        assert_eq!(parse_cache_limit(Some(0)).unwrap(), None);
        assert_eq!(parse_cache_limit(Some(1 << 30)).unwrap(), Some(1 << 30));
        assert_eq!(
            parse_cache_limit(Some(1024)).unwrap_err().code,
            "invalid_input"
        );
        let io = cache_error(CacheError::Io(std::io::ErrorKind::PermissionDenied));
        assert_eq!(io.code, "removal_failed");
        assert!(!io.message.contains('/'));
        assert_eq!(
            cache_error(CacheError::NotTorrentCourse).code,
            "not_torrent_course"
        );
    }

    #[test]
    fn insufficient_space_reports_sizes_in_gigabytes() {
        let error = insufficient_space(3 << 30, Some((3 << 30) / 2));
        assert_eq!(error.code, "insufficient_space");
        assert!(error.message.contains("3.0 GB more"), "{}", error.message);
        assert!(
            error.message.contains("only 1.5 GB is free"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("leaves 1.0 GB free"),
            "{}",
            error.message
        );
    }

    #[test]
    fn progress_of_unknown_courses_is_an_error() {
        let library =
            CourseLibrary::open_in_memory(learning_application::ImportLimits::default()).unwrap();
        assert_eq!(
            progress(&library, "course").unwrap_err().code,
            "course_not_found"
        );
    }

    #[test]
    fn note_dtos_are_tagged_and_anchors_parse_strictly() {
        let conflict = serde_json::to_value(NoteChangeDto::Conflict {
            current_revision: 4,
        })
        .unwrap();
        assert_eq!(
            conflict,
            serde_json::json!({ "outcome": "conflict", "currentRevision": 4 })
        );
        assert_eq!(
            serde_json::to_value(NoteChangeDto::NotFound).unwrap(),
            serde_json::json!({ "outcome": "notFound" })
        );
        assert!(
            serde_json::from_value::<Anchor>(
                serde_json::json!({ "kind": "videoTime", "resourceId": "r", "seconds": 3 })
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<Anchor>(
                serde_json::json!({ "kind": "lesson", "path": "/etc" })
            )
            .is_err()
        );
        assert_eq!(
            note_error(LibraryError::NoteTarget("x")).code,
            "note_target_invalid"
        );
    }

    #[test]
    fn document_errors_have_codes_and_no_paths() {
        assert_eq!(
            document_error(DocumentError::Changed).code,
            "document_changed"
        );
        assert_eq!(
            document_error(DocumentError::TooLarge).code,
            "document_too_large"
        );
        let library = document_error(DocumentError::Library(LibraryError::NoteTarget("x")));
        assert_eq!(library.code, "library_query_failed");
        assert!(!library.message.contains('/'));
    }

    #[test]
    fn rate_limits_are_validated() {
        assert_eq!(
            parse_rate_limits(None, Some(0)).unwrap(),
            RateLimits::default()
        );
        assert_eq!(
            parse_rate_limits(Some(64), None)
                .unwrap()
                .as_kib_per_second(),
            (Some(64), None)
        );
        assert_eq!(
            parse_rate_limits(Some(1), None).unwrap_err().code,
            "invalid_input"
        );
        assert_eq!(
            parse_rate_limits(None, Some(u32::MAX)).unwrap_err().code,
            "invalid_input"
        );
    }

    #[test]
    fn reachability_input_is_validated() {
        assert_eq!(
            parse_reachability(None, true).unwrap(),
            PeerReachability::new(None, true).unwrap()
        );
        assert_eq!(
            parse_reachability(Some(1023), false).unwrap_err().code,
            "invalid_input"
        );
    }

    #[test]
    fn torrent_import_snapshots_map_to_path_free_dtos() {
        let dto = TorrentImportDto::from(&TorrentImportSnapshot {
            job_id: "import_1".to_owned(),
            sequence: 3,
            stage: TorrentImportStage::FetchingManifest,
            info_hash: Some("ab".repeat(20)),
            manifest_bytes: Some((512, 2048)),
            course_id: None,
            title: None,
            failure: None,
        });
        assert_eq!(dto.stage, "fetching_manifest");
        assert_eq!(dto.manifest_verified_bytes, Some(512));
        assert_eq!(dto.manifest_total_bytes, Some(2048));

        let failed = TorrentImportDto::from(&TorrentImportSnapshot {
            job_id: "import_2".to_owned(),
            sequence: 4,
            stage: TorrentImportStage::Failed,
            info_hash: None,
            manifest_bytes: None,
            course_id: None,
            title: None,
            failure: Some(TorrentImportFailure {
                code: "metadata_timeout",
                message: "no peer sent the torrent metadata".to_owned(),
            }),
        });
        assert_eq!(failed.stage, "failed");
        assert_eq!(failed.failure.unwrap().code, "metadata_timeout");
        assert_eq!(source_name(None), "local");
        assert_eq!(source_name(Some("ab")), "torrent");
    }

    #[test]
    fn sharing_status_maps_local_metrics_and_hides_engine_details() {
        let idle = CourseSharingDto::new(
            CourseSharingStatus {
                course_id: "course_one".to_owned(),
                sharing: false,
                transfer: None,
            },
            true,
            NetworkProfile::LocalNetwork,
        );
        assert_eq!(idle.state, "inactive");
        assert_eq!(idle.network_profile, "local_network");
        assert_eq!(idle.uploaded_bytes, 0);

        let shared = CourseSharingDto::new(
            CourseSharingStatus {
                course_id: "course_one".to_owned(),
                sharing: true,
                transfer: Some(learning_application::TransferStats {
                    info_hash: learning_application::InfoHash::parse(&"a".repeat(40)).unwrap(),
                    state: TransferState::Seeding,
                    verified_bytes: 10,
                    total_bytes: 100,
                    uploaded_bytes: 4096,
                    upload_bytes_per_second: 512,
                    downloaded_bytes: 0,
                    connected_peers: 2,
                    file_verified_bytes: vec![10, 0],
                    failure: None,
                }),
            },
            true,
            NetworkProfile::Internet,
        );
        assert_eq!(shared.state, "ready");
        assert_eq!(shared.connected_peers, 2);
        assert_eq!(shared.uploaded_bytes, 4096);
        assert_eq!(shared.upload_bytes_per_second, 512);

        let failed = sharing_error(SharingError::Engine(
            learning_application::EngineError::Engine("error opening /home/learner/x".to_owned()),
        ));
        assert_eq!(failed.code, "sharing_failed");
        assert!(!failed.message.contains("/home/"));
    }

    #[test]
    fn local_import_failures_never_name_local_directories() {
        let io = || std::io::Error::other("cannot open /home/learner/course");
        let failures = [
            LibraryError::Import(ImportError::Io {
                operation: "opening course.json",
                source: io(),
            }),
            LibraryError::PackageLocation(io()),
            LibraryError::Storage(learning_storage::StorageError::MissingCapability(
                "/home/learner/library.sqlite3",
            )),
        ];
        for failure in failures {
            let error = local_import_error(failure);
            assert!(!error.message.contains("/home/"), "{}", error.message);
        }

        let read = local_import_error(LibraryError::Import(ImportError::Io {
            operation: "opening course.json",
            source: io(),
        }));
        assert_eq!(read.code, "course_import_failed");
        assert_eq!(
            read.message,
            "the course folder could not be read while opening course.json"
        );

        let missing = local_import_error(LibraryError::Import(ImportError::MissingResource {
            resource_id: "intro".to_owned(),
            package_path: "media/intro.mp4".to_owned(),
        }));
        assert_eq!(missing.code, "course_import_failed");
        assert!(missing.message.contains("media/intro.mp4"));
    }

    #[test]
    fn magnets_are_trimmed_and_bounded() {
        let hash = "ab".repeat(20);
        assert_eq!(
            validate_magnet(&format!("  magnet:?xt=urn:btih:{hash}\n")).unwrap(),
            format!("magnet:?xt=urn:btih:{hash}")
        );
        assert!(validate_magnet("https://example.com/course.torrent").is_err());
        let long = format!(
            "magnet:?xt=urn:btih:{hash}&tr={}",
            "a".repeat(MAX_MAGNET_BYTES)
        );
        assert_eq!(validate_magnet(&long).unwrap_err().code, "invalid_magnet");
    }

    #[test]
    fn oversized_torrent_files_are_refused_without_reading_them_whole() {
        let directory =
            std::env::temp_dir().join(format!("zeron-learner-torrent-file-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let small = directory.join("small.torrent");
        std::fs::write(&small, b"d4:infode").unwrap();
        assert_eq!(read_torrent_file(&small).unwrap(), b"d4:infode");
        let large = directory.join("large.torrent");
        let file = std::fs::File::create(&large).unwrap();
        file.set_len(MAX_TORRENT_FILE_BYTES + 1).unwrap();
        assert_eq!(
            read_torrent_file(&large).unwrap_err().code,
            "torrent_file_too_large"
        );
        let missing = read_torrent_file(&directory.join("missing.torrent")).unwrap_err();
        assert_eq!(missing.code, "torrent_file_unreadable");
        assert!(!missing.message.contains(directory.to_str().unwrap()));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
