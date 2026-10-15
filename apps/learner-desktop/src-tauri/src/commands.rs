use std::sync::{Arc, Mutex};

use course_format::ResourceKind;
use learning_application::{CancellationFlag, CourseImportReceipt, CourseLibrary};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadinessDto {
    state: &'static str,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: String,
}

#[tauri::command]
pub(crate) fn readiness() -> ReadinessDto {
    ReadinessDto { state: "ready" }
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
        library
            .manifest(&course_id)
            .map(|manifest| manifest.map(CourseDetailDto::from))
            .map_err(|error| command_error("course_detail_failed", error))
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

impl From<course_format::CourseManifest> for CourseDetailDto {
    fn from(manifest: course_format::CourseManifest) -> Self {
        Self {
            course_id: manifest.course_id,
            course_version: manifest.version,
            title: manifest.title,
            description: manifest.description,
            publisher_name: manifest.publisher.name,
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
