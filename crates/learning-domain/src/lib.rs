//! Learner-owned learning rules: progress now, annotations later. Pure logic
//! with no storage, engine, or UI dependency.

mod progress;

pub use progress::{COMPLETION_RATIO, CourseProgress, LessonStatus, watched_enough};
