//! Builder drag-and-drop: dropped paths stay here; the UI sees a path-free
//! summary and an opaque token it can apply to a draft.

use std::sync::Arc;

use course_format::ResourceKind;
use publisher_application::{
    DropInbox, DropLimits, DropOutcome, DropPlan, PlannedFile, PublisherDrafts, SkippedEntry,
    plan_drop,
};
use serde::{Deserialize, Serialize};
use tauri::{DragDropEvent, Emitter, Manager, State, Window};

use super::{
    PublishState,
    commands::{CommandError, DraftDetailDto, lock_drafts, run_blocking, service_error},
};

/// `{ active }` while files are dragged over the window.
pub(crate) const DRAG_EVENT: &str = "builder-drag";
/// A [`DropSummaryDto`] once dropped files have been scanned.
pub(crate) const DROP_EVENT: &str = "builder-drop";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DragDto {
    active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DropSummaryDto {
    token: String,
    /// Drop point in CSS pixels of the window, for finding the lesson under it.
    x: f64,
    y: f64,
    modules: Vec<DropModuleDto>,
    loose_files: Vec<DropFileDto>,
    skipped: Vec<SkippedDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DropModuleDto {
    title: String,
    lessons: Vec<DropFileDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DropFileDto {
    title: String,
    file_name: String,
    kind: ResourceKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkippedDto {
    name: String,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyDropDto {
    draft: DraftDetailDto,
    modules_added: usize,
    lessons_added: usize,
    files_attached: usize,
    skipped: Vec<SkippedDto>,
}

/// Where to put loose files; folders always become new modules.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DropTargetInput {
    lesson_id: Option<String>,
}

/// Translate native drag-and-drop events. Scanning runs off the event loop.
pub(crate) fn handle_drag_drop(window: &Window, event: &DragDropEvent) {
    match event {
        DragDropEvent::Enter { .. } => {
            let _ = window.emit(DRAG_EVENT, DragDto { active: true });
        }
        DragDropEvent::Leave => {
            let _ = window.emit(DRAG_EVENT, DragDto { active: false });
        }
        DragDropEvent::Drop { paths, position } => {
            let _ = window.emit(DRAG_EVENT, DragDto { active: false });
            let scale = window.scale_factor().unwrap_or(1.0);
            let point = position.to_logical::<f64>(scale);
            let paths = paths.clone();
            let window = window.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let plan = plan_drop(&paths, DropLimits::default());
                let inbox = Arc::clone(&window.state::<PublishState>().drops);
                if let Ok(summary) = summarize(&inbox, plan, point.x, point.y) {
                    let _ = window.emit(DROP_EVENT, summary);
                }
            });
        }
        _ => {}
    }
}

fn summarize(
    inbox: &DropInbox,
    plan: DropPlan,
    x: f64,
    y: f64,
) -> Result<DropSummaryDto, CommandError> {
    let summary = DropSummaryDto {
        token: String::new(),
        x,
        y,
        modules: plan
            .modules
            .iter()
            .map(|module| DropModuleDto {
                title: module.title.clone(),
                lessons: module.lessons.iter().map(DropFileDto::from).collect(),
            })
            .collect(),
        loose_files: plan.loose_files.iter().map(DropFileDto::from).collect(),
        skipped: plan.skipped.iter().map(SkippedDto::from).collect(),
    };
    let token = inbox
        .insert(plan)
        .map_err(|error| service_error("drop_failed", error))?;
    Ok(DropSummaryDto { token, ..summary })
}

/// Apply a scanned drop to a draft as one revision.
#[tauri::command]
pub(crate) async fn apply_drop(
    draft_id: String,
    expected_revision: u64,
    token: String,
    target: DropTargetInput,
    state: State<'_, PublishState>,
) -> Result<ApplyDropDto, CommandError> {
    let drafts = Arc::clone(&state.drafts);
    let inbox = Arc::clone(&state.drops);
    run_blocking(move || {
        apply(
            &mut *lock_drafts(&drafts)?,
            &inbox,
            &draft_id,
            expected_revision,
            &token,
            target.lesson_id.as_deref(),
        )
    })
    .await
}

/// Forget a scanned drop the publisher dismissed.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects owned command arguments"
)]
pub(crate) fn discard_drop(token: String, state: State<'_, PublishState>) {
    state.drops.discard(&token);
}

fn apply(
    drafts: &mut PublisherDrafts,
    inbox: &DropInbox,
    draft_id: &str,
    expected_revision: u64,
    token: &str,
    lesson_id: Option<&str>,
) -> Result<ApplyDropDto, CommandError> {
    let plan = inbox.plan(token).ok_or_else(|| {
        CommandError::new("drop_expired", "this drop expired; drop the files again")
    })?;
    let outcome = drafts
        .apply_drop(draft_id, expected_revision, &plan, lesson_id)
        .map_err(|error| service_error("drop_failed", error))?;
    inbox.discard(token);
    Ok(ApplyDropDto::from(outcome))
}

impl From<&PlannedFile> for DropFileDto {
    fn from(file: &PlannedFile) -> Self {
        Self {
            title: file.title.clone(),
            file_name: file.file_name.clone(),
            kind: file.kind,
        }
    }
}

impl From<&SkippedEntry> for SkippedDto {
    fn from(entry: &SkippedEntry) -> Self {
        Self {
            name: entry.name.clone(),
            reason: entry.reason.as_str(),
        }
    }
}

impl From<DropOutcome> for ApplyDropDto {
    fn from(outcome: DropOutcome) -> Self {
        Self {
            draft: DraftDetailDto::from(outcome.draft),
            modules_added: outcome.modules_added,
            lessons_added: outcome.lessons_added,
            files_attached: outcome.files_attached,
            skipped: outcome.skipped.iter().map(SkippedDto::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn summaries_and_outcomes_are_path_free_and_tokens_are_single_use() {
        let directory =
            std::env::temp_dir().join(format!("zeron-admin-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(directory.join("Course/extra")).unwrap();
        std::fs::write(directory.join("Course/lesson10.mp4"), b"x").unwrap();
        std::fs::write(directory.join("Course/lesson2.mp4"), b"x").unwrap();
        std::fs::write(directory.join("Course/notes.txt"), b"x").unwrap();
        std::fs::write(directory.join("handout.pdf"), b"%PDF-1.7").unwrap();
        let plan = plan_drop(
            &[directory.join("Course"), directory.join("handout.pdf")],
            DropLimits::default(),
        );
        let inbox = DropInbox::new(4, Duration::from_mins(1));
        let summary = summarize(&inbox, plan, 120.5, 40.0).unwrap();
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json["token"].as_str().unwrap().starts_with("drop_"));
        assert_eq!(json["x"], 120.5);
        assert_eq!(json["modules"][0]["title"], "Course");
        assert_eq!(json["modules"][0]["lessons"][0]["title"], "lesson2");
        assert_eq!(json["modules"][0]["lessons"][1]["kind"], "video");
        assert_eq!(json["looseFiles"][0]["fileName"], "handout.pdf");
        let reasons: Vec<_> = json["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["reason"].as_str().unwrap())
            .collect();
        assert_eq!(reasons, ["nested_folder", "unsupported_type"]);
        assert!(!json.to_string().contains(&*directory.to_string_lossy()));

        let mut drafts = PublisherDrafts::open_in_memory().unwrap();
        let draft_id = drafts.create().unwrap().draft.draft_id;
        let token = summary.token.clone();
        let stale = apply(&mut drafts, &inbox, &draft_id, 5, &token, None).unwrap_err();
        assert_eq!(stale.code(), "revision_conflict");
        let applied = apply(&mut drafts, &inbox, &draft_id, 0, &token, None).unwrap();
        let applied = serde_json::to_value(applied).unwrap();
        assert_eq!(applied["modulesAdded"], 1);
        assert_eq!(applied["lessonsAdded"], 2);
        assert_eq!(applied["draft"]["revision"], 1);
        assert_eq!(applied["skipped"].as_array().unwrap().len(), 3);
        assert_eq!(applied["skipped"][2]["reason"], "no_target_lesson");
        assert!(!applied.to_string().contains(&*directory.to_string_lossy()));
        let reused = apply(&mut drafts, &inbox, &draft_id, 1, &token, None).unwrap_err();
        assert_eq!(reused.code(), "drop_expired");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn drop_targets_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<DropTargetInput>(serde_json::json!({ "lessonId": null }))
                .is_ok()
        );
        assert!(
            serde_json::from_value::<DropTargetInput>(serde_json::json!({ "path": "/etc" }))
                .is_err()
        );
    }
}
