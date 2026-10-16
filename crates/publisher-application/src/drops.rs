//! Turn files and folders dropped onto the Builder into draft structure.
//!
//! Dropped paths are untrusted. Scanning is bounded, never follows symbolic
//! links, skips hidden entries, and reports everything it leaves out by name
//! only. Each top-level folder becomes a module whose media and PDF files
//! become lessons in natural name order. A folder holding only folders is a
//! course folder: each folder inside it becomes a module. Loose files become
//! lessons of one new module, or attach to a lesson the publisher chooses.

use std::{
    cmp::Ordering,
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use course_format::ResourceKind;
use course_publisher::{DraftLesson, DraftModule, DraftResource};

use crate::{
    DraftServiceError, MAX_TITLE_CHARS, PublisherDrafts, StoredDraft, edit_error, opaque_id,
    sources,
};

/// Bounds on one drop, so a dropped home folder cannot stall the Builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropLimits {
    /// Directory entries read across the whole drop.
    pub max_entries: usize,
    /// Files accepted as lessons or attachments.
    pub max_files: usize,
    /// Top-level folders accepted as modules.
    pub max_folders: usize,
}

impl Default for DropLimits {
    fn default() -> Self {
        Self {
            max_entries: 5_000,
            max_files: 1_000,
            max_folders: 100,
        }
    }
}

/// Why a dropped entry was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Its name starts with a dot.
    Hidden,
    /// Symbolic links are never followed.
    SymbolicLink,
    /// Folders are imported at most one level below a course folder.
    NestedFolder,
    /// Inside a folder, only video, audio, and PDF files become lessons.
    UnsupportedType,
    /// Not a regular file or folder, or it could not be read.
    Unreadable,
    /// The folder held no file that could become a lesson.
    NoLessons,
    /// The drop exceeded [`DropLimits`].
    LimitReached,
}

impl SkipReason {
    /// Stable name used over IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::SymbolicLink => "symbolic_link",
            Self::NestedFolder => "nested_folder",
            Self::UnsupportedType => "unsupported_type",
            Self::Unreadable => "unreadable",
            Self::NoLessons => "no_lessons",
            Self::LimitReached => "limit_reached",
        }
    }
}

/// An entry left out of a drop, named relative to what was dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    /// For example `Course/notes.txt`; never an absolute path.
    pub name: String,
    /// Why it was left out.
    pub reason: SkipReason,
}

/// A file that will become a lesson or an attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Suggested lesson title derived from the file name.
    pub title: String,
    /// Final file-name component only.
    pub file_name: String,
    /// Kind inferred from the extension.
    pub kind: ResourceKind,
    source: PathBuf,
}

/// A dropped folder that will become a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedModule {
    /// The folder name.
    pub title: String,
    /// One lesson per accepted file, in natural name order.
    pub lessons: Vec<PlannedFile>,
}

/// What a drop would add, computed before anything is changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropPlan {
    /// Folders that become modules, in natural name order.
    pub modules: Vec<PlannedModule>,
    /// Files dropped directly: lessons of one new module, or attachments
    /// of one chosen lesson.
    pub loose_files: Vec<PlannedFile>,
    /// Title for the module loose files become: their folder's name when
    /// they share one.
    pub loose_module_title: String,
    /// Everything left out.
    pub skipped: Vec<SkippedEntry>,
}

impl DropPlan {
    /// Whether nothing in the drop can be added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.loose_files.is_empty()
    }
}

/// Scan dropped paths into a [`DropPlan`] without changing anything.
#[must_use]
pub fn plan_drop(paths: &[PathBuf], limits: DropLimits) -> DropPlan {
    let mut plan = DropPlan::default();
    let mut budget = Budget {
        limits,
        entries: 0,
        files: 0,
    };
    let mut top: Vec<&PathBuf> = paths.iter().collect();
    top.sort_by(|a, b| natural_cmp(&display_name(a), &display_name(b)));
    for path in top {
        let name = display_name(path);
        if !budget.entry() {
            plan.skip(name, SkipReason::LimitReached);
            continue;
        }
        if name.starts_with('.') {
            plan.skip(name, SkipReason::Hidden);
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(path) else {
            plan.skip(name, SkipReason::Unreadable);
            continue;
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            plan.skip(name, SkipReason::SymbolicLink);
        } else if file_type.is_dir() {
            if plan.modules.len() >= limits.max_folders {
                plan.skip(name, SkipReason::LimitReached);
                continue;
            }
            let (module, folders) = scan_folder(path, &name, &name, &mut budget);
            if module.module.lessons.is_empty() && !folders.is_empty() {
                // A course folder: each folder inside it is a module.
                plan.skipped.extend(module.skipped);
                for (folder, folder_name) in folders {
                    if plan.modules.len() >= limits.max_folders {
                        plan.skip(format!("{name}/{folder_name}"), SkipReason::LimitReached);
                        continue;
                    }
                    let relative = format!("{name}/{folder_name}");
                    let (inner, nested) =
                        scan_folder(&folder, &folder_name, &relative, &mut budget);
                    plan.skipped.extend(inner.skipped);
                    for (_, nested_name) in nested {
                        plan.skip(
                            format!("{relative}/{nested_name}"),
                            SkipReason::NestedFolder,
                        );
                    }
                    if inner.module.lessons.is_empty() {
                        plan.skip(relative, SkipReason::NoLessons);
                    } else {
                        plan.modules.push(inner.module);
                    }
                }
            } else {
                plan.skipped.extend(module.skipped);
                for (_, folder_name) in folders {
                    plan.skip(format!("{name}/{folder_name}"), SkipReason::NestedFolder);
                }
                if module.module.lessons.is_empty() {
                    plan.skip(name, SkipReason::NoLessons);
                } else {
                    plan.modules.push(module.module);
                }
            }
        } else if file_type.is_file() {
            if budget.file() {
                plan.loose_files.push(planned(path, name));
            } else {
                plan.skip(name, SkipReason::LimitReached);
            }
        } else {
            plan.skip(name, SkipReason::Unreadable);
        }
    }
    plan.loose_module_title = loose_module_title(paths, &plan.loose_files);
    plan
}

/// The shared folder name of the loose files, or "New module".
fn loose_module_title(paths: &[PathBuf], loose: &[PlannedFile]) -> String {
    let mut parents = loose.iter().filter_map(|file| {
        paths
            .iter()
            .find(|path| path.as_path() == file.source.as_path())
            .and_then(|path| path.parent())
    });
    let first = parents.next();
    match first {
        Some(parent) if parents.all(|other| other == parent) => parent.file_name().map_or_else(
            || "New module".to_owned(),
            |name| clean_title(&name.to_string_lossy(), false),
        ),
        _ => "New module".to_owned(),
    }
}

struct Budget {
    limits: DropLimits,
    entries: usize,
    files: usize,
}

impl Budget {
    fn entry(&mut self) -> bool {
        self.entries += 1;
        self.entries <= self.limits.max_entries
    }

    fn file(&mut self) -> bool {
        self.files += 1;
        self.files <= self.limits.max_files
    }
}

impl DropPlan {
    fn skip(&mut self, name: String, reason: SkipReason) {
        self.skipped.push(SkippedEntry { name, reason });
    }
}

/// A scanned folder: the module its files make, and what it left out.
struct ScannedFolder {
    module: PlannedModule,
    skipped: Vec<SkippedEntry>,
}

/// Scan one folder: its media and PDF files become lessons. Folders inside
/// it are returned with their names, for the caller to decide about.
/// `relative` names the folder in skip reports.
fn scan_folder(
    folder: &Path,
    folder_name: &str,
    relative: &str,
    budget: &mut Budget,
) -> (ScannedFolder, Vec<(PathBuf, String)>) {
    let mut scanned = ScannedFolder {
        module: PlannedModule {
            title: clean_title(folder_name, false),
            lessons: Vec::new(),
        },
        skipped: Vec::new(),
    };
    let mut folders = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return (scanned, folders);
    };
    let mut children = Vec::new();
    for entry in entries {
        if !budget.entry() {
            scanned.skipped.push(SkippedEntry {
                name: format!("{relative}/…"),
                reason: SkipReason::LimitReached,
            });
            break;
        }
        let Ok(entry) = entry else { continue };
        children.push(entry.path());
    }
    children.sort_by(|a, b| natural_cmp(&display_name(a), &display_name(b)));
    for child in children {
        let name = display_name(&child);
        let child_relative = format!("{relative}/{name}");
        let reason = if name.starts_with('.') {
            Some(SkipReason::Hidden)
        } else {
            match fs::symlink_metadata(&child).map(|metadata| metadata.file_type()) {
                Ok(kind) if kind.is_symlink() => Some(SkipReason::SymbolicLink),
                Ok(kind) if kind.is_dir() => {
                    folders.push((child, name));
                    continue;
                }
                Ok(kind) if kind.is_file() => {
                    if !is_lesson_kind(sources::classify(&child).0) {
                        Some(SkipReason::UnsupportedType)
                    } else if budget.file() {
                        None
                    } else {
                        Some(SkipReason::LimitReached)
                    }
                }
                _ => Some(SkipReason::Unreadable),
            }
        };
        match reason {
            Some(reason) => scanned.skipped.push(SkippedEntry {
                name: child_relative,
                reason,
            }),
            None => scanned.module.lessons.push(planned(&child, name)),
        }
    }
    (scanned, folders)
}

/// Only video, audio, and PDF files become lessons of their own.
const fn is_lesson_kind(kind: ResourceKind) -> bool {
    matches!(
        kind,
        ResourceKind::Video | ResourceKind::Audio | ResourceKind::Pdf
    )
}

fn planned(path: &Path, file_name: String) -> PlannedFile {
    PlannedFile {
        title: clean_title(&file_name, true),
        kind: sources::classify(path).0,
        file_name,
        source: path.to_path_buf(),
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "(unnamed)".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A readable lesson or module title from a file or folder name: known media
/// extensions are removed (repeatedly, for names like `a.mp4.mp4`), a
/// leading ordinal such as `01 - ` is dropped, and the result is bounded.
#[must_use]
pub fn clean_title(name: &str, strip_extensions: bool) -> String {
    let mut title = name.trim();
    if strip_extensions {
        while let Some((stem, extension)) = title.rsplit_once('.') {
            let known = !stem.is_empty()
                && !matches!(
                    sources::classify(Path::new(&format!("x.{extension}"))).0,
                    ResourceKind::Other
                );
            if !known {
                break;
            }
            title = stem.trim_end();
        }
    }
    let without_ordinal = title
        .trim_start_matches(|character: char| character.is_ascii_digit())
        .trim_start_matches([' ', '.', ')', '-', '_', ':'])
        .trim_start();
    if without_ordinal.len() < title.len()
        && title.starts_with(|character: char| character.is_ascii_digit())
        && without_ordinal.chars().any(char::is_alphabetic)
    {
        title = without_ordinal;
    }
    let title: String = title
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_TITLE_CHARS)
        .collect();
    let title = title.trim();
    if title.is_empty() {
        "Untitled".to_owned()
    } else {
        title.to_owned()
    }
}

/// Compare names so that `lesson2` sorts before `lesson10`.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                        digits.push(digit);
                    }
                    digits
                };
                let (x, y) = (take(&mut left), take(&mut right));
                let (x_trimmed, y_trimmed) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let ordering = x_trimmed
                    .len()
                    .cmp(&y_trimmed.len())
                    .then_with(|| x_trimmed.cmp(y_trimmed));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(x), Some(y)) => {
                let ordering = x.to_lowercase().cmp(y.to_lowercase());
                if ordering != Ordering::Equal {
                    return ordering;
                }
                left.next();
                right.next();
            }
        }
    }
}

/// What applying a drop changed.
#[derive(Debug, Clone)]
pub struct DropOutcome {
    /// The saved draft at its new revision.
    pub draft: StoredDraft,
    /// Modules created from folders.
    pub modules_added: usize,
    /// Lessons created inside those modules.
    pub lessons_added: usize,
    /// Loose files attached to the chosen lesson.
    pub files_attached: usize,
    /// Entries left out while planning or applying.
    pub skipped: Vec<SkippedEntry>,
}

impl PublisherDrafts {
    /// Apply a drop plan as one draft revision: folders become modules, and
    /// loose files attach to `lesson_id` or, without one, become the lessons
    /// of one new module. Files that vanished since the drop
    /// are skipped. Nothing is copied; sources are recorded like picked files.
    ///
    /// # Errors
    /// Rejects stale revisions, an unknown target lesson, and plans that
    /// would add nothing.
    pub fn apply_drop(
        &mut self,
        draft_id: &str,
        expected_revision: u64,
        plan: &DropPlan,
        lesson_id: Option<&str>,
    ) -> Result<DropOutcome, DraftServiceError> {
        let mut draft = self.require(draft_id)?;
        let mut skipped = plan.skipped.clone();
        let mut modules = Vec::new();
        let mut lessons_added = 0;
        for planned_module in &plan.modules {
            let mut lessons = Vec::new();
            for file in &planned_module.lessons {
                let name = format!("{}/{}", planned_module.title, file.file_name);
                let Some(resource) = resource_for(file, &name, &mut skipped)? else {
                    continue;
                };
                lessons.push(DraftLesson {
                    id: opaque_id("lesson")?,
                    title: file.title.clone(),
                    estimated_duration_seconds: None,
                    resources: vec![resource],
                    paid: false,
                });
            }
            if lessons.is_empty() {
                continue;
            }
            lessons_added += lessons.len();
            modules.push(DraftModule {
                id: opaque_id("module")?,
                title: planned_module.title.clone(),
                lessons,
            });
        }
        let mut attachments = Vec::new();
        let mut loose_lessons = Vec::new();
        for file in &plan.loose_files {
            if lesson_id.is_none() && !is_lesson_kind(file.kind) {
                skipped.push(SkippedEntry {
                    name: file.file_name.clone(),
                    reason: SkipReason::UnsupportedType,
                });
            } else if let Some(resource) = resource_for(file, &file.file_name, &mut skipped)? {
                if lesson_id.is_some() {
                    attachments.push(resource);
                } else {
                    loose_lessons.push(DraftLesson {
                        id: opaque_id("lesson")?,
                        title: file.title.clone(),
                        estimated_duration_seconds: None,
                        resources: vec![resource],
                        paid: false,
                    });
                }
            }
        }
        if !loose_lessons.is_empty() {
            lessons_added += loose_lessons.len();
            modules.push(DraftModule {
                id: opaque_id("module")?,
                title: plan.loose_module_title.clone(),
                lessons: loose_lessons,
            });
        }
        if modules.is_empty() && attachments.is_empty() {
            return Err(DraftServiceError::InvalidInput {
                field: "drop".to_owned(),
                message: "contains nothing that can be added",
            });
        }
        let modules_added = modules.len();
        let files_attached = attachments.len();
        let target = lesson_id
            .filter(|_| !attachments.is_empty())
            .map(|id| (id, attachments));
        draft
            .import(expected_revision, modules, target)
            .map_err(edit_error)?;
        Ok(DropOutcome {
            draft: self.save(expected_revision, draft)?,
            modules_added,
            lessons_added,
            files_attached,
            skipped,
        })
    }
}

fn resource_for(
    file: &PlannedFile,
    name: &str,
    skipped: &mut Vec<SkippedEntry>,
) -> Result<Option<DraftResource>, DraftServiceError> {
    let regular = fs::symlink_metadata(&file.source).is_ok_and(|metadata| metadata.is_file());
    let source = fs::canonicalize(&file.source).ok().filter(|_| regular);
    let Some(source) = source else {
        skipped.push(SkippedEntry {
            name: name.to_owned(),
            reason: SkipReason::Unreadable,
        });
        return Ok(None);
    };
    let (kind, media_type) = sources::classify(&source);
    let id = opaque_id("resource")?;
    Ok(Some(DraftResource {
        package_path: format!(
            "resources/{id}/{}",
            sources::package_file_name(&file.file_name)
        ),
        duration_seconds: course_publisher::inspect_media(&source, media_type).duration_seconds,
        id,
        kind,
        source_path: source,
        media_type: media_type.to_owned(),
        language: None,
    }))
}

/// Short-lived drop plans waiting for the publisher to choose a target.
/// Only opaque tokens leave the backend.
pub struct DropInbox {
    pending: Mutex<VecDeque<(String, Instant, DropPlan)>>,
    capacity: usize,
    lifetime: Duration,
}

impl Default for DropInbox {
    fn default() -> Self {
        Self::new(4, Duration::from_mins(10))
    }
}

impl DropInbox {
    /// An inbox keeping at most `capacity` plans for `lifetime` each.
    #[must_use]
    pub fn new(capacity: usize, lifetime: Duration) -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            lifetime,
        }
    }

    /// Keep a plan and return its opaque token, evicting the oldest plans
    /// beyond capacity.
    ///
    /// # Errors
    /// Returns an error when randomness fails.
    pub fn insert(&self, plan: DropPlan) -> Result<String, DraftServiceError> {
        let token = opaque_id("drop")?;
        let mut pending = self.lock();
        self.expire(&mut pending);
        pending.push_back((token.clone(), Instant::now(), plan));
        while pending.len() > self.capacity {
            pending.pop_front();
        }
        Ok(token)
    }

    /// The plan for `token`, if it has not expired or been discarded.
    #[must_use]
    pub fn plan(&self, token: &str) -> Option<DropPlan> {
        let mut pending = self.lock();
        self.expire(&mut pending);
        pending
            .iter()
            .find(|(candidate, _, _)| candidate == token)
            .map(|(_, _, plan)| plan.clone())
    }

    /// Forget a plan after it was applied or dismissed.
    pub fn discard(&self, token: &str) {
        self.lock().retain(|(candidate, _, _)| candidate != token);
    }

    fn expire(&self, pending: &mut VecDeque<(String, Instant, DropPlan)>) {
        pending.retain(|(_, created, _)| created.elapsed() < self.lifetime);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(String, Instant, DropPlan)>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
