//! Learner-owned learning rules: progress now, annotations later. Pure logic
//! with no storage, engine, or UI dependency.

mod notes;
mod progress;

pub use notes::{Anchor, MAX_NOTE_BYTES, NoteBody, NoteError, Region};
pub use progress::{COMPLETION_RATIO, CourseProgress, LessonStatus, watched_enough};
