//! Learner-owned learning rules: progress, notes, drawings, transcripts, and
//! moving annotations to a new course version. Pure logic
//! with no storage, engine, or UI dependency.

mod drawings;
mod migration;
mod notes;
mod progress;
mod search;
mod transcript;

pub use drawings::{
    DrawingError, DrawingFile, MAX_FILE_DATA_URL_BYTES, MAX_FILES_PER_DRAWING, MAX_SCENE_BYTES,
    Scene,
};
pub use migration::{
    MatchReason, OutlineLesson, OutlineResource, remap_anchor, resource_map, suggest_lesson,
};
pub use notes::{
    Anchor, HighlightColor, HighlightStyle, MAX_HIGHLIGHT_RECTS, MAX_NOTE_BYTES, MAX_QUOTE_CHARS,
    NoteBody, NoteError, Region, TextHighlight,
};
pub use progress::{COMPLETION_RATIO, CourseProgress, LessonStatus, watched_enough};
pub use search::{fts_query, text_passages, transcript_passages};
pub use transcript::{Cue, MAX_TRANSCRIPT_BYTES, TranscriptError, parse_transcript};
