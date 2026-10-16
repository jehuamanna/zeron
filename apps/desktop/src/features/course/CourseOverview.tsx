import { BookOpen, ChevronRight, Clock, Layers, Magnet, Play } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import { AnnotationMovesCard } from "../migration/AnnotationMovesCard";
import { LessonStatusIcon, ProgressBar, type useCourseProgress } from "../progress/progress";
import { PublisherBadge } from "../publisher/PublisherBadge";
import { CourseSharingCard } from "../sharing/CourseSharingCard";
import type { CourseDetail } from "../../services/library";
import { formatDuration, formatTotal, lessonDuration, type Lesson, type PlacedLesson } from "./lessons";

/** The course's front page: what it is, who published it, what is inside, and where to pick up. */
export function CourseOverview({ course, placed, progress, next, onStart, onOpenLesson }: {
  course: CourseDetail;
  placed: PlacedLesson[];
  progress: ReturnType<typeof useCourseProgress>;
  next: PlacedLesson | null;
  onStart: (placed: PlacedLesson) => void;
  onOpenLesson: (lesson: Lesson) => void;
}) {
  const torrent = course.source === "torrent";
  const seconds = placed.reduce(
    (total, { lesson }) => total + lesson.resources.reduce((sum, resource) => sum + (resource.durationSeconds ?? 0), 0),
    0,
  );
  const started = (progress.progress?.completedLessons ?? 0) > 0 || (next !== null && progress.statusOf(next.lesson.id) === "in_progress");
  // The module holding the next lesson starts open.
  const [open, setOpen] = useState<Set<string>>(() => new Set(next ? [next.moduleId] : course.modules[0] ? [course.modules[0].id] : []));
  const number = new Map(placed.map((item) => [item.lesson.id, item.index + 1]));
  const toggle = (moduleId: string) =>
    setOpen((current) => {
      const updated = new Set(current);
      if (updated.has(moduleId)) updated.delete(moduleId);
      else updated.add(moduleId);
      return updated;
    });

  return (
    <div className="relative h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8 sm:py-10">
        <p className="mb-2 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Course overview</p>
        <h1 className="font-display text-3xl font-bold leading-tight tracking-tight sm:text-4xl">{course.title}</h1>
        <p className="mt-3 max-w-2xl text-[15px] leading-relaxed text-muted">{course.description ?? `Published by ${course.publisherName}`}</p>

        <dl className="mt-5 flex flex-wrap gap-x-6 gap-y-2 text-sm text-fg-soft">
          <div className="flex items-center gap-1.5"><Layers size={15} className="text-subtle" /><dt className="sr-only">Modules</dt><dd>{course.modules.length} {course.modules.length === 1 ? "module" : "modules"}</dd></div>
          <div className="flex items-center gap-1.5"><BookOpen size={15} className="text-subtle" /><dt className="sr-only">Lessons</dt><dd>{placed.length} {placed.length === 1 ? "lesson" : "lessons"}</dd></div>
          {seconds > 0 ? <div className="flex items-center gap-1.5"><Clock size={15} className="text-subtle" /><dt className="sr-only">Video length</dt><dd>{formatTotal(seconds)} of video</dd></div> : null}
        </dl>

        {/* Who published it, where it comes from, and sharing: one quiet row. */}
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <PublisherBadge compact courseId={course.courseId} publisher={course.publisher} />
          {torrent ? (
            <span
              className="inline-flex items-center gap-1.5 rounded-full border border-line bg-surface px-2.5 py-1 text-xs font-medium text-muted"
              title="Lessons download while you watch, and only data verified against the torrent is played. Each lesson shows how much of it is on this computer."
            >
              <Magnet size={13} /> Streams from a torrent
            </span>
          ) : null}
          {torrent ? <CourseSharingCard compact courseId={course.courseId} /> : null}
        </div>

        <div className="mt-6 rounded-2xl border border-line bg-surface p-5 shadow-panel">
          {progress.progress ? (
            <ProgressBar label="Course progress" completed={progress.progress.completedLessons} total={progress.progress.totalLessons} />
          ) : null}
          {next ? (
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-xs text-subtle">{started ? "Up next" : "Start with"} · Lesson {next.index + 1} of {placed.length}</p>
                <p className="truncate font-display text-lg font-bold">{next.lesson.title}</p>
              </div>
              <Button onClick={() => onStart(next)}>
                <Play size={15} /> {started ? "Continue" : "Start course"}
              </Button>
            </div>
          ) : null}
        </div>

        <AnnotationMovesCard course={course} />

        {course.modules.length > 0 ? (
          <section aria-label="Syllabus" className="mt-8">
            <div className="mb-3 flex items-baseline justify-between">
              <h2 className="text-sm font-semibold">What’s inside</h2>
              <button
                type="button"
                className="text-xs text-muted hover:text-fg"
                onClick={() => setOpen(open.size === course.modules.length ? new Set() : new Set(course.modules.map((module) => module.id)))}
              >
                {open.size === course.modules.length ? "Collapse all" : "Expand all"}
              </button>
            </div>
            <ol className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
              {course.modules.map((module, index) => {
                const expanded = open.has(module.id);
                const done = module.lessons.filter((lesson) => progress.statusOf(lesson.id) === "completed").length;
                return (
                  <li key={module.id}>
                    <button
                      type="button"
                      aria-expanded={expanded}
                      onClick={() => toggle(module.id)}
                      className="flex w-full items-center gap-3 px-4 py-2.5 text-left text-sm hover:bg-white/5"
                    >
                      <ChevronRight size={15} className={`shrink-0 text-subtle transition-transform ${expanded ? "rotate-90" : ""}`} />
                      <span className="w-5 shrink-0 tabular-nums text-subtle">{index + 1}</span>
                      <span className="min-w-0 flex-1 truncate font-medium">{module.title}</span>
                      <span className="shrink-0 text-xs tabular-nums text-subtle">
                        {done ? `${done}/` : ""}{module.lessons.length} {module.lessons.length === 1 ? "lesson" : "lessons"}
                      </span>
                    </button>
                    {expanded ? (
                      <ol className="border-t border-line bg-bg/40 py-1">
                        {module.lessons.map((lesson) => {
                          const duration = lessonDuration(lesson);
                          return (
                            <li key={lesson.id}>
                              <button type="button" onClick={() => onOpenLesson(lesson)} className="flex w-full items-center gap-3 py-1.5 pl-12 pr-4 text-left text-sm text-fg-soft hover:bg-white/5 hover:text-fg">
                                <LessonStatusIcon status={progress.statusOf(lesson.id)} />
                                <span className="w-6 shrink-0 tabular-nums text-subtle">{number.get(lesson.id)}.</span>
                                <span className="min-w-0 flex-1 truncate">{lesson.title}</span>
                                {duration ? <span className="shrink-0 text-xs tabular-nums text-subtle">{formatDuration(duration)}</span> : null}
                              </button>
                            </li>
                          );
                        })}
                      </ol>
                    ) : null}
                  </li>
                );
              })}
            </ol>
          </section>
        ) : null}
      </div>
    </div>
  );
}
