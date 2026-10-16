import { BookOpen, Clock, Layers, Play } from "lucide-react";

import { Button } from "../../components/ui/button";
import { AnnotationMovesCard } from "../migration/AnnotationMovesCard";
import { ProgressBar, type useCourseProgress } from "../progress/progress";
import { PublisherBadge } from "../publisher/PublisherBadge";
import { CourseSharingCard } from "../sharing/CourseSharingCard";
import type { CourseDetail } from "../../services/library";
import { formatTotal, type PlacedLesson } from "./lessons";

/** The course's front page: what it is, who published it, and where to pick up. */
export function CourseOverview({ course, placed, progress, next, onStart }: {
  course: CourseDetail;
  placed: PlacedLesson[];
  progress: ReturnType<typeof useCourseProgress>;
  next: PlacedLesson | null;
  onStart: (placed: PlacedLesson) => void;
}) {
  const torrent = course.source === "torrent";
  const seconds = placed.reduce(
    (total, { lesson }) => total + lesson.resources.reduce((sum, resource) => sum + (resource.durationSeconds ?? 0), 0),
    0,
  );
  const started = (progress.progress?.completedLessons ?? 0) > 0 || (next !== null && progress.statusOf(next.lesson.id) === "in_progress");

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8 sm:py-10">
        <p className="mb-2 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Course overview</p>
        <h1 className="font-display text-3xl font-semibold leading-tight tracking-tight sm:text-4xl">{course.title}</h1>
        <p className="mt-3 max-w-2xl text-[15px] leading-relaxed text-muted">{course.description ?? `Published by ${course.publisherName}`}</p>

        <dl className="mt-6 flex flex-wrap gap-x-6 gap-y-2 text-sm text-fg-soft">
          <div className="flex items-center gap-1.5"><Layers size={15} className="text-subtle" /><dt className="sr-only">Modules</dt><dd>{course.modules.length} {course.modules.length === 1 ? "module" : "modules"}</dd></div>
          <div className="flex items-center gap-1.5"><BookOpen size={15} className="text-subtle" /><dt className="sr-only">Lessons</dt><dd>{placed.length} {placed.length === 1 ? "lesson" : "lessons"}</dd></div>
          {seconds > 0 ? <div className="flex items-center gap-1.5"><Clock size={15} className="text-subtle" /><dt className="sr-only">Video length</dt><dd>{formatTotal(seconds)} of video</dd></div> : null}
        </dl>

        <div className="mt-6 rounded-2xl border border-line bg-surface p-5 shadow-panel">
          {progress.progress ? (
            <ProgressBar label="Course progress" completed={progress.progress.completedLessons} total={progress.progress.totalLessons} />
          ) : null}
          {next ? (
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-xs text-subtle">{started ? "Up next" : "Start with"} · Lesson {next.index + 1} of {placed.length}</p>
                <p className="truncate font-display text-lg font-semibold">{next.lesson.title}</p>
              </div>
              <Button onClick={() => onStart(next)}>
                <Play size={15} /> {started ? "Continue" : "Start course"}
              </Button>
            </div>
          ) : null}
        </div>

        <PublisherBadge courseId={course.courseId} publisher={course.publisher} />
        {torrent ? (
          <p className="mt-5 max-w-2xl rounded-xl border border-amber-300/30 bg-amber-300/10 px-4 py-3 text-sm text-amber-100">
            This course streams from a torrent. Lessons download while you watch, and only data verified against the torrent is played. Each lesson shows how much of it is on this computer.
          </p>
        ) : null}
        {torrent ? <CourseSharingCard courseId={course.courseId} /> : null}
        <AnnotationMovesCard course={course} />

        {course.modules.length > 0 ? (
          <section aria-label="Syllabus" className="mt-8">
            <h2 className="mb-3 text-sm font-semibold">What’s inside</h2>
            <ol className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
              {course.modules.map((module, index) => (
                <li key={module.id} className="flex items-center justify-between gap-3 px-4 py-2.5 text-sm">
                  <span className="min-w-0 truncate"><span className="mr-2 tabular-nums text-subtle">{index + 1}</span>{module.title}</span>
                  <span className="shrink-0 text-xs text-subtle">{module.lessons.length} {module.lessons.length === 1 ? "lesson" : "lessons"}</span>
                </li>
              ))}
            </ol>
          </section>
        ) : null}
      </div>
    </div>
  );
}
