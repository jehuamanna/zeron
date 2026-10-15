//! Learner-owned learning rules: progress now, annotations later. Pure logic
//! with no storage, engine, or UI dependency.

mod drawings;
mod notes;
mod progress;

pub use drawings::{
    DrawingError, DrawingFile, MAX_FILE_DATA_URL_BYTES, MAX_FILES_PER_DRAWING, MAX_SCENE_BYTES,
    Scene,
};
pub use notes::{Anchor, MAX_NOTE_BYTES, NoteBody, NoteError, Region};
pub use progress::{COMPLETION_RATIO, CourseProgress, LessonStatus, watched_enough};
