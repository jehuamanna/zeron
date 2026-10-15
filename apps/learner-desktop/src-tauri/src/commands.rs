use std::{
    io::Read as _,
    sync::{Arc, Mutex},
};

use course_format::ResourceKind;
use learning_application::{
    CancellationFlag, CourseImportReceipt, CourseLibrary, CourseSharingStatus, NetworkProfile,
    SharingError, TorrentImportSnapshot, TorrentImportStage, TorrentSource, TransferState,
};
use media_gateway::FileResource;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;

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
        library
            .courses()
            .map(|courses| {
                courses
                    .into_iter()
                    .map(|course| CourseSummaryDto {
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
    let root = selected.into_path().map_err(|error| CommandError {
        code: "unsupported_folder_location",
        message: error.to_string(),
    })?;
    let library = Arc::clone(&state.library);
    run_blocking(move || {
        let mut library = lock_library(&library)?;
        library
            .import_local_folder(&root, &CancellationFlag::default())
            .map(ImportReceiptDto::from)
            .map(Some)
            .map_err(|error| command_error("course_import_failed", error))
    })
    .await
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
        torrents.profile,
        torrents.imports.is_some(),
        torrents.download_port,
        torrents.sharing_port,
    );
    let configured = run_blocking(move || {
        lock_library(&library)?
            .network_profile()
            .map_err(|error| command_error("settings_failed", error))
    })
    .await?;
    Ok(NetworkSettingsDto {
        configured_profile: configured.name(),
        active_profile: active.name(),
        torrent_engine: if available { "ready" } else { "unavailable" },
        download_port,
        sharing_port,
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

#[tauri::command]
pub(crate) async fn register_media(
    course_id: String,
    resource_id: String,
    state: State<'_, AppState>,
) -> Result<MediaRegistrationDto, CommandError> {
    let library = Arc::clone(&state.library);
    let resource = run_blocking(move || {
        let library = lock_library(&library)?;
        if library
            .torrent_info_hash(&course_id)
            .map_err(|error| command_error("media_lookup_failed", error))?
            .is_some()
        {
            return Err(CommandError {
                code: "media_not_downloaded",
                message: "this course came from a torrent and its media is not downloaded yet"
                    .to_owned(),
            });
        }
        library
            .local_resource(&course_id, &resource_id)
            .map_err(|error| command_error("media_lookup_failed", error))?
            .ok_or_else(|| CommandError {
                code: "media_not_found",
                message: "the selected media resource is unavailable".to_owned(),
            })
    })
    .await?;
    state
        .media_gateway
        .register(FileResource {
            path: resource.path,
            media_type: resource.media_type,
            byte_length: resource.byte_length,
        })
        .await
        .map(|registration| MediaRegistrationDto {
            url: registration.url,
            media_type: registration.media_type,
            byte_length: registration.byte_length,
        })
        .map_err(|error| command_error("media_registration_failed", error))
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
