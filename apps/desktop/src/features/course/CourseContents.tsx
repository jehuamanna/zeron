import { BookOpen, File, NotebookPen, Play } from "lucide-react";

import { AvailabilityLabel, formatSize, percentOf, KeepOfflineToggle, PauseTransfersButton, useCourseDownloads } from "../downloads/downloads";
import { LessonStatusIcon, MarkCompleteButton, ProgressBar, useCourseProgress } from "../progress/progress";
import type { CourseDetail, ResourceAvailability } from "../../services/library";
import { formatDuration, isReadable, lessonDuration, type Lesson, type PlacedLesson } from "./lessons";

/** Which resource the stage shows, so the list can mark it. */
export type Showing = { kind: "video" | "pdf" | "text"; resourceId: string } | null;

/** The course's modules and lessons, with progress and, for torrent courses, downloads. */
export function CourseContents({ course, placed, activeLessonId, showing, opening, torrent, availability, offline, progress, lowDisk, onOpenResource, onOpenLesson, onNotes }: {
  course: CourseDetail;
  placed: PlacedLesson[];
  activeLessonId: string | null;
  showing: Showing;
  /** A video is being registered; its buttons wait. */
  opening: boolean;
  torrent: boolean;
  availability: Map<string, ResourceAvailability>;
  offline: ReturnType<typeof useCourseDownloads>;
  progress: ReturnType<typeof useCourseProgress>;
  lowDisk: boolean;
  onOpenResource: (lesson: Lesson, resourceId: string) => void;
  onOpenLesson: (lesson: Lesson) => void;
  onNotes: (lesson: Lesson) => void;
}) {
  const number = new Map(placed.map((item) => [item.lesson.id, item.index + 1]));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 space-y-3 border-b border-line px-4 py-3">
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-sm font-semibold">Course contents</h2>
          <span className="text-xs text-subtle">{placed.length} {placed.length === 1 ? "lesson" : "lessons"}</span>
        </div>
        {progress.progress ? (
          <ProgressBar label="Course progress" completed={progress.progress.completedLessons} total={progress.progress.totalLessons} />
        ) : null}
        {torrent ? (
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <PauseTransfersButton
                paused={offline.downloads?.paused ?? false}
                disabled={offline.pause.isPending || !offline.downloads}
                onToggle={(paused) => offline.pause.mutate(paused)}
              />
              <KeepOfflineToggle
                label="Keep the whole course offline"
                kept={offline.downloads?.course ?? false}
                disabled={offline.change.isPending || !offline.downloads}
                onToggle={(keep) => offline.change.mutate({ scope: { kind: "course" }, keep })}
              />
              <button
                type="button"
                onClick={() => offline.remove.mutate(undefined)}
                disabled={offline.remove.isPending}
                title="Remove downloaded lessons that are not kept offline"
                className="inline-flex items-center gap-1 rounded-md bg-white/5 px-2 py-1 text-xs text-fg-soft hover:bg-white/10 disabled:opacity-60"
              >
                {offline.remove.isPending ? "Freeing space…" : "Free up space"}
              </button>
            </div>
            {offline.remove.isSuccess ? (
              <p role="status" className="text-xs text-muted">
                Freed {formatSize(offline.remove.data.freedBytes)}. Lessons kept offline were not touched.
              </p>
            ) : null}
            {lowDisk ? <p role="alert" className="text-xs text-amber-200">Downloads are paused because less than 1 GB is free on this disk. Free up space to continue.</p> : null}
            {offline.downloads?.paused ? <p className="text-xs text-amber-200">Downloads and sharing are paused. Lessons already on this computer still play.</p> : null}
            {offline.error ? <p role="alert" className="text-xs text-red-300">{offline.error}</p> : null}
          </div>
        ) : null}
      </div>

      <div className="relative min-h-0 flex-1 overflow-y-auto px-2 py-2">
        {course.modules.map((module) => (
          <section key={module.id} aria-label={`Module ${module.title}`} className="mb-3">
            <div className="sticky top-0 z-[1] flex items-center justify-between gap-2 bg-surface/95 px-2 py-1.5 backdrop-blur">
              <h3 className="text-[11px] font-bold uppercase tracking-wider text-subtle">{module.title}</h3>
              {torrent ? (
                <KeepOfflineToggle
                  compact
                  label={`Keep module ${module.title} offline`}
                  kept={offline.downloads?.modules.includes(module.id) ?? false}
                  inherited={offline.downloads?.course ?? false}
                  disabled={offline.change.isPending || !offline.downloads}
                  onToggle={(keep) => offline.change.mutate({ scope: { kind: "module", id: module.id }, keep })}
                />
              ) : null}
            </div>
            <ol className="space-y-0.5">
              {module.lessons.map((lesson) => {
                const active = lesson.id === activeLessonId;
                const duration = lessonDuration(lesson);
                return (
                  <li
                    key={lesson.id}
                    data-active={active || undefined}
                    className={`group flex items-start gap-2 rounded-lg px-2 py-2 transition ${active ? "bg-accent/10" : "hover:bg-white/5"}`}
                  >
                    <LessonStatusIcon status={progress.statusOf(lesson.id)} />
                    <div className="min-w-0 flex-1">
                      <button
                        type="button"
                        onClick={() => onOpenLesson(lesson)}
                        className={`block w-full text-left text-sm leading-snug ${active ? "font-semibold text-fg" : "text-fg-soft hover:text-fg"}`}
                      >
                        <span className="mr-1 tabular-nums text-subtle">{number.get(lesson.id)}.</span>
                        {lesson.title}
                      </button>
                      <div className="mt-1 flex flex-wrap items-center gap-1">
                        {duration ? <span className="mr-1 text-[11px] tabular-nums text-subtle">{formatDuration(duration)}</span> : null}
                        {lesson.resources.map((resource) => {
                          const current = showing?.resourceId === resource.id;
                          const chip = `inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium disabled:opacity-50 ${current ? "bg-accent text-on-accent" : "bg-white/5 text-muted hover:bg-white/10 hover:text-fg"}`;
                          if (resource.kind === "video") {
                            const item = availability.get(resource.id);
                            return (
                              <span key={resource.id} className="inline-flex flex-wrap items-center gap-1.5">
                                <button type="button" onClick={() => onOpenResource(lesson, resource.id)} disabled={opening} aria-current={current ? "true" : undefined} className={chip}>
                                  <Play size={10} /> {current ? "Playing" : "Play video"}
                                  {torrent ? <span className="opacity-75">· {percentOf(item)}% downloaded</span> : null}
                                </button>
                                {torrent ? <AvailabilityLabel item={item} /> : null}
                                {torrent && item?.kept === false && item.verifiedBytes > 0 && !current ? (
                                  <button
                                    type="button"
                                    aria-label={`Remove download of ${lesson.title}`}
                                    onClick={() => offline.remove.mutate([resource.id])}
                                    disabled={offline.remove.isPending}
                                    className="text-[11px] text-subtle underline-offset-2 hover:text-red-300 hover:underline disabled:opacity-60"
                                  >
                                    Remove download
                                  </button>
                                ) : null}
                              </span>
                            );
                          }
                          if (resource.kind === "pdf") {
                            return (
                              <button type="button" key={resource.id} aria-current={current ? "true" : undefined} onClick={() => onOpenResource(lesson, resource.id)} className={chip}>
                                <File size={10} /> {current ? "Reading" : "Open PDF"}
                              </button>
                            );
                          }
                          if (isReadable(resource)) {
                            return (
                              <button type="button" key={resource.id} aria-current={current ? "true" : undefined} onClick={() => onOpenResource(lesson, resource.id)} className={chip}>
                                <BookOpen size={10} /> {current ? "Reading" : "Read"}
                              </button>
                            );
                          }
                          return (
                            <span key={resource.id} className="inline-flex items-center gap-1 rounded-md bg-white/5 px-1.5 py-0.5 text-[11px] text-subtle">
                              <File size={10} /> {resource.kind}
                            </span>
                          );
                        })}
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-0.5">
                      <button
                        type="button"
                        aria-label={`Notes for ${lesson.title}`}
                        aria-pressed={active}
                        title="Notes"
                        onClick={() => onNotes(lesson)}
                        className={`grid size-6 place-items-center rounded-md ${active ? "text-accent-text" : "text-faint hover:bg-white/10 hover:text-fg"}`}
                      >
                        <NotebookPen size={14} />
                      </button>
                      <MarkCompleteButton
                        compact
                        lessonTitle={lesson.title}
                        completed={progress.statusOf(lesson.id) === "completed"}
                        disabled={progress.mark.isPending}
                        onToggle={(completed) => progress.mark.mutate({ lessonId: lesson.id, completed })}
                      />
                      {torrent ? (
                        <KeepOfflineToggle
                          compact
                          label={`Keep lesson ${lesson.title} offline`}
                          kept={offline.downloads?.lessons.includes(lesson.id) ?? false}
                          inherited={(offline.downloads?.course ?? false) || (offline.downloads?.modules.includes(module.id) ?? false)}
                          disabled={offline.change.isPending || !offline.downloads}
                          onToggle={(keep) => offline.change.mutate({ scope: { kind: "lesson", id: lesson.id }, keep })}
                        />
                      ) : null}
                    </div>
                  </li>
                );
              })}
            </ol>
          </section>
        ))}
        {course.modules.length === 0 ? <p className="px-2 py-6 text-center text-sm text-muted">This course has no lessons yet.</p> : null}
      </div>
    </div>
  );
}
