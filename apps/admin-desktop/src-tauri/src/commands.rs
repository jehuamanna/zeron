use std::sync::{Arc, Mutex};

use course_publisher::DraftValidationIssue;
use publisher_application::{
    DraftServiceError, DraftSummary, MetadataEdit, PublisherDrafts, StoredDraft,
};
use serde::{Deserialize, Serialize};
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
    resource_count: usize,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ValidationIssueDto {
    code: &'static str,
    field: String,
    message: String,
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

fn validate(
    drafts: &PublisherDrafts,
    draft_id: &str,
) -> Result<Vec<ValidationIssueDto>, CommandError> {
    drafts
        .validate(draft_id)
        .map(|issues| issues.into_iter().map(ValidationIssueDto::from).collect())
        .map_err(|error| service_error("draft_validation_failed", error))
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
        .map_err(|error| CommandError::new("worker_failed", error))?
}

fn lock_drafts(
    drafts: &Arc<Mutex<PublisherDrafts>>,
) -> Result<std::sync::MutexGuard<'_, PublisherDrafts>, CommandError> {
    drafts.lock().map_err(|_| {
        CommandError::new(
            "storage_unavailable",
            "the publisher storage worker is unavailable",
        )
    })
}

fn service_error(code: &'static str, error: DraftServiceError) -> CommandError {
    match error {
        DraftServiceError::RevisionConflict { actual, .. } => CommandError {
            current_revision: Some(actual),
            ..CommandError::new("revision_conflict", error)
        },
        DraftServiceError::NotFound { .. } => CommandError::new("draft_not_found", error),
        DraftServiceError::InvalidInput { ref field, .. } => CommandError {
            field: Some(field.clone()),
            ..CommandError::new("invalid_input", error)
        },
        DraftServiceError::IdGeneration(_) | DraftServiceError::Storage(_) => {
            CommandError::new(code, error)
        }
    }
}

impl CommandError {
    fn new(code: &'static str, message: impl std::fmt::Display) -> Self {
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
    fn validation_issues_are_path_free() {
        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = create(&mut drafts).unwrap().draft_id;
        let issues = validate(&drafts, &draft_id).unwrap();
        assert!(issues.iter().any(|issue| issue.code == "empty_course"));
        let error = validate(&drafts, "draft_missing").unwrap_err();
        assert_eq!(error.code, "draft_not_found");
    }

    #[test]
    fn detail_dto_counts_resources_without_exposing_source_paths() {
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
        assert_eq!(json["modules"][0]["lessons"][0]["resourceCount"], 1);
        assert_eq!(json["updatedAtUnixSeconds"], 7);
        let text = json.to_string();
        assert!(!text.contains("/home/"));
        assert!(!text.contains("sourcePath"));
        assert!(!text.contains("packagePath"));
    }
}
