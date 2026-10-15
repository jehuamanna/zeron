//! Lesson and course progress.

/// Share of a video that counts as watching it.
pub const COMPLETION_RATIO: f64 = 0.9;

/// Where a learner is with one lesson.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessonStatus {
    /// Never opened.
    NotStarted,
    /// Opened but not finished.
    InProgress,
    /// Finished, by watching or by the learner's say-so.
    Completed,
}

impl LessonStatus {
    /// Status from what was recorded for a lesson.
    #[must_use]
    pub const fn from_record(started: bool, completed: bool) -> Self {
        if completed {
            Self::Completed
        } else if started {
            Self::InProgress
        } else {
            Self::NotStarted
        }
    }

    /// Stable name used over IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

/// Whether watching up to `position` of `duration` seconds finishes a video:
/// at least [`COMPLETION_RATIO`] of it, or within the last ten seconds.
#[must_use]
pub fn watched_enough(position_seconds: f64, duration_seconds: f64) -> bool {
    duration_seconds.is_finite()
        && duration_seconds > 0.0
        && position_seconds.is_finite()
        && (position_seconds >= duration_seconds * COMPLETION_RATIO
            || duration_seconds - position_seconds <= 10.0)
}

/// Completed lessons out of all lessons of a course.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CourseProgress {
    /// Lessons completed.
    pub completed: usize,
    /// Lessons in the course.
    pub total: usize,
}

impl CourseProgress {
    /// Count completed lessons among `statuses`.
    #[must_use]
    pub fn of(statuses: impl IntoIterator<Item = LessonStatus>) -> Self {
        statuses
            .into_iter()
            .fold(Self::default(), |progress, status| Self {
                completed: progress.completed + usize::from(status == LessonStatus::Completed),
                total: progress.total + 1,
            })
    }

    /// Whole percent complete, rounded down; an empty course is 0%.
    #[must_use]
    pub fn percent(self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        u8::try_from(self.completed.min(self.total) * 100 / self.total).unwrap_or(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_video_counts_as_watched_near_its_end() {
        assert!(!watched_enough(0.0, 600.0));
        assert!(!watched_enough(500.0, 600.0));
        assert!(watched_enough(540.0, 600.0));
        assert!(watched_enough(15.0, 20.0));
        assert!(watched_enough(11.0, 20.0));
        assert!(!watched_enough(5.0, 20.0));
        assert!(!watched_enough(f64::NAN, 600.0));
        assert!(!watched_enough(1.0, 0.0));
        assert!(!watched_enough(1.0, f64::INFINITY));
    }

    #[test]
    fn statuses_and_course_progress() {
        assert_eq!(
            LessonStatus::from_record(false, false),
            LessonStatus::NotStarted
        );
        assert_eq!(
            LessonStatus::from_record(true, false),
            LessonStatus::InProgress
        );
        assert_eq!(
            LessonStatus::from_record(false, true),
            LessonStatus::Completed
        );
        assert_eq!(LessonStatus::InProgress.as_str(), "in_progress");
        let progress = CourseProgress::of([
            LessonStatus::Completed,
            LessonStatus::InProgress,
            LessonStatus::NotStarted,
        ]);
        assert_eq!(
            progress,
            CourseProgress {
                completed: 1,
                total: 3
            }
        );
        assert_eq!(progress.percent(), 33);
        assert_eq!(CourseProgress::default().percent(), 0);
        assert_eq!(
            CourseProgress::of([LessonStatus::Completed; 4]).percent(),
            100
        );
    }
}
