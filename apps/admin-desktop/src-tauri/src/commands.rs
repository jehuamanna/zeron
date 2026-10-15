use std::sync::{Arc, Mutex};

use course_publisher::{DraftCourse, DraftMetadata};
use publisher_storage::{DraftSummary, PublisherStorage};
use serde::Serialize;
use tauri::State;

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
    resource_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    code: &'static str,
    message: String,
}

#[tauri::command]
pub(crate) async fn list_drafts(
    state: State<'_, AppState>,
) -> Result<Vec<DraftSummaryDto>, CommandError> {
    let storage = Arc::clone(&state.storage);
    run_blocking(move || list(&*lock_storage(&storage)?)).await
}

#[tauri::command]
pub(crate) async fn create_draft(
    state: State<'_, AppState>,
) -> Result<DraftDetailDto, CommandError> {
    let storage = Arc::clone(&state.storage);
    run_blocking(move || create(&mut *lock_storage(&storage)?)).await
}

#[tauri::command]
pub(crate) async fn get_draft(
    draft_id: String,
    state: State<'_, AppState>,
) -> Result<Option<DraftDetailDto>, CommandError> {
    let storage = Arc::clone(&state.storage);
    run_blocking(move || detail(&*lock_storage(&storage)?, &draft_id)).await
}

fn list(storage: &PublisherStorage) -> Result<Vec<DraftSummaryDto>, CommandError> {
    storage
        .list_drafts()
        .map(|drafts| drafts.into_iter().map(DraftSummaryDto::from).collect())
        .map_err(|error| command_error("draft_list_failed", error))
}

fn create(storage: &mut PublisherStorage) -> Result<DraftDetailDto, CommandError> {
    let draft = new_draft().map_err(|error| command_error("draft_id_generation_failed", error))?;
    storage
        .create_draft(&draft)
        .map(|summary| DraftDetailDto::from_draft(draft, summary.updated_at_unix_seconds))
        .map_err(|error| command_error("draft_create_failed", error))
}

fn detail(
    storage: &PublisherStorage,
    draft_id: &str,
) -> Result<Option<DraftDetailDto>, CommandError> {
    let Some(draft) = storage
        .draft(draft_id)
        .map_err(|error| command_error("draft_lookup_failed", error))?
    else {
        return Ok(None);
    };
    let updated_at_unix_seconds = storage
        .list_drafts()
        .map_err(|error| command_error("draft_lookup_failed", error))?
        .into_iter()
        .find(|summary| summary.draft_id == draft_id)
        .map_or(0, |summary| summary.updated_at_unix_seconds);
    Ok(Some(DraftDetailDto::from_draft(
        draft,
        updated_at_unix_seconds,
    )))
}

fn new_draft() -> Result<DraftCourse, getrandom::Error> {
    Ok(DraftCourse {
        draft_id: opaque_id("draft")?,
        revision: 0,
        metadata: DraftMetadata {
            course_id: opaque_id("course")?,
            version: "0.1.0".to_owned(),
            title: "Untitled course".to_owned(),
            description: None,
            publisher_name: "Local publisher".to_owned(),
            language: "en".to_owned(),
            category: None,
            tags: Vec::new(),
            prerequisites: Vec::new(),
            license_name: None,
        },
        modules: Vec::new(),
    })
}

fn opaque_id(prefix: &str) -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!("{prefix}_{hex}"))
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

impl DraftDetailDto {
    fn from_draft(draft: DraftCourse, updated_at_unix_seconds: u64) -> Self {
        Self {
            draft_id: draft.draft_id,
            course_id: draft.metadata.course_id,
            course_version: draft.metadata.version,
            title: draft.metadata.title,
            revision: draft.revision,
            updated_at_unix_seconds,
            description: draft.metadata.description,
            publisher_name: draft.metadata.publisher_name,
            language: draft.metadata.language,
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
                            resource_count: lesson.resources.len(),
                        })
                        .collect(),
                })
                .collect(),
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

fn lock_storage(
    storage: &Arc<Mutex<PublisherStorage>>,
) -> Result<std::sync::MutexGuard<'_, PublisherStorage>, CommandError> {
    storage.lock().map_err(|_| CommandError {
        code: "storage_unavailable",
        message: "the publisher storage worker is unavailable".to_owned(),
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
    use std::path::PathBuf;

    use course_format::ResourceKind;
    use course_publisher::{DraftLesson, DraftModule, DraftResource};

    use super::*;

    #[test]
    fn new_draft_uses_defaults_and_opaque_ids() {
        let first = new_draft().unwrap();
        let second = new_draft().unwrap();
        assert_eq!(first.revision, 0);
        assert_eq!(first.metadata.version, "0.1.0");
        assert_eq!(first.metadata.title, "Untitled course");
        assert_eq!(first.metadata.publisher_name, "Local publisher");
        assert_eq!(first.metadata.language, "en");
        assert!(first.modules.is_empty());
        assert!(
            first
                .validate()
                .iter()
                .any(|issue| issue.code == "empty_course")
        );
        for (id, prefix) in [
            (&first.draft_id, "draft_"),
            (&first.metadata.course_id, "course_"),
        ] {
            let hex = id.strip_prefix(prefix).unwrap();
            assert_eq!(hex.len(), 32);
            assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        assert_ne!(first.draft_id, second.draft_id);
        assert_ne!(first.metadata.course_id, second.metadata.course_id);
    }

    #[test]
    fn created_draft_round_trips_through_storage() {
        let mut storage = PublisherStorage::open_in_memory().unwrap();
        let created = create(&mut storage).unwrap();
        let listed = list(&storage).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].draft_id, created.draft_id);
        assert_eq!(listed[0].revision, 0);
        let loaded = detail(&storage, &created.draft_id).unwrap().unwrap();
        assert_eq!(loaded.course_id, created.course_id);
        assert_eq!(
            loaded.updated_at_unix_seconds,
            created.updated_at_unix_seconds
        );
        assert!(detail(&storage, "draft_missing").unwrap().is_none());
    }

    #[test]
    fn detail_dto_counts_resources_without_exposing_source_paths() {
        let mut draft = new_draft().unwrap();
        draft.modules.push(DraftModule {
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
        let json = serde_json::to_value(DraftDetailDto::from_draft(draft, 7)).unwrap();
        assert_eq!(json["modules"][0]["lessons"][0]["resourceCount"], 1);
        assert_eq!(json["updatedAtUnixSeconds"], 7);
        let text = json.to_string();
        assert!(!text.contains("/home/"));
        assert!(!text.contains("sourcePath"));
        assert!(!text.contains("packagePath"));
    }
}
