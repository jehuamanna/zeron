use std::sync::{Arc, Mutex};

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
