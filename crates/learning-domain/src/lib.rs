//! Learner-owned learning rules: progress, notes, drawings, and transcripts. Pure logic
//! with no storage, engine, or UI dependency.

mod drawings;
mod notes;
mod progress;
mod search;
mod transcript;

pub use drawings::{
    DrawingError, DrawingFile, MAX_FILE_DATA_URL_BYTES, MAX_FILES_PER_DRAWING, MAX_SCENE_BYTES,
    Scene,
};
pub use notes::{Anchor, MAX_NOTE_BYTES, NoteBody, NoteError, Region};
pub use progress::{COMPLETION_RATIO, CourseProgress, LessonStatus, watched_enough};
pub use search::{fts_query, text_passages, transcript_passages};
pub use transcript::{Cue, MAX_TRANSCRIPT_BYTES, TranscriptError, parse_transcript};
