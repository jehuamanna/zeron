import type { CourseDetail, LessonStatus } from "../../services/library";

export type Lesson = CourseDetail["modules"][number]["lessons"][number];
export type Resource = Lesson["resources"][number];

/** A lesson with its place in the course, for "Lesson 3 of 12" and next/previous. */
export interface PlacedLesson {
  lesson: Lesson;
  moduleId: string;
  moduleTitle: string;
  /** Zero-based position across the whole course. */
  index: number;
}

export function placeLessons(course: CourseDetail): PlacedLesson[] {
  let index = 0;
  return course.modules.flatMap((module) =>
    module.lessons.map((lesson) => ({ lesson, moduleId: module.id, moduleTitle: module.title, index: index++ })),
  );
}

export const isReadable = (resource: Resource) => resource.kind === "markdown" || resource.kind === "text";

/** What opening a lesson shows first: its video, then its PDF, then its reading. */
export function primaryResource(lesson: Lesson): Resource | null {
  return (
    lesson.resources.find((resource) => resource.kind === "video") ??
    lesson.resources.find((resource) => resource.kind === "pdf") ??
    lesson.resources.find(isReadable) ??
    null
  );
}

/** The lesson to continue with: the first one started, else the first not done. */
export function nextToLearn(lessons: PlacedLesson[], statusOf: (lessonId: string) => LessonStatus): PlacedLesson | null {
  return (
    lessons.find((placed) => statusOf(placed.lesson.id) === "in_progress") ??
    lessons.find((placed) => statusOf(placed.lesson.id) === "not_started") ??
    lessons[0] ??
    null
  );
}

export function lessonDuration(lesson: Lesson): number | null {
  const seconds = lesson.resources.reduce((total, resource) => total + (resource.durationSeconds ?? 0), 0);
  return seconds > 0 ? seconds : null;
}

export function formatDuration(seconds: number): string {
  const whole = Math.round(seconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
  return `${minutes}:${String(whole % 60).padStart(2, "0")}`;
}

/** "3 h 20 min" for course totals. */
export function formatTotal(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
}

export const resourceLabel: Record<string, string> = { video: "Video", pdf: "PDF", markdown: "Reading", text: "Reading" };
