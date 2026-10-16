import { BookOpen, Eraser, File, NotebookPen, Play } from "lucide-react";

import { AvailabilityLabel, formatSize, KeepOfflineToggle, PauseTransfersButton, percentOf, useCourseDownloads } from "../downloads/downloads";
import { LessonStatusIcon, MarkCompleteButton, useCourseProgress } from "../progress/progress";
import type { CourseDetail, ResourceAvailability } from "../../services/library";
import { formatDuration, isReadable, lessonDuration, type Lesson, type PlacedLesson } from "./lessons";

/** Which resource the stage shows, so the list can mark it. */
export type Showing = { kind: "video" | "pdf" | "text"; resourceId: string } | null;

const iconButton = "grid size-6 shrink-0 place-items-center rounded-md disabled:opacity-50";

/** The course's modules and lessons, one line each, with progress and, for torrent courses, downloads. */
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
  const done = progress.progress?.completedLessons ?? 0;
  const total = progress.progress?.totalLessons ?? placed.length;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 border-b border-line px-3 py-2.5">
        <div className="flex items-center gap-2">
          <h2 className="text-[13px] font-semibold">Contents</h2>
          <span className="text-[11px] text-subtle">{done}/{total} done</span>
          {torrent ? (
            <div className="ml-auto flex items-center">
              <PauseTransfersButton
                compact
                paused={offline.downloads?.paused ?? false}
                disabled={offline.pause.isPending || !offline.downloads}
                onToggle={(paused) => offline.pause.mutate(paused)}
              />
              <KeepOfflineToggle
                compact
                label="Keep the whole course offline"
                kept={offline.downloads?.course ?? false}
                disabled={offline.change.isPending || !offline.downloads}
                onToggle={(keep) => offline.change.mutate({ scope: { kind: "course" }, keep })}
              />
              <button
                type="button"
                aria-label="Free up space"
                title="Free up space: remove downloads that are not kept offline"
                onClick={() => offline.remove.mutate(undefined)}
                disabled={offline.remove.isPending}
                className={`${iconButton} size-7 text-muted hover:bg-white/10 hover:text-fg`}
              >
                <Eraser size={14} />
              </button>
            </div>
          ) : null}
        </div>
        {progress.progress ? (
          <div
            role="progressbar"
            aria-label="Course progress"
            aria-valuemin={0}
            aria-valuemax={total}
            aria-valuenow={done}
            className="mt-2 h-1 overflow-hidden rounded-full bg-white/10"
          >
            <div className="h-full rounded-full bg-accent" style={{ width: `${total ? Math.floor((done / total) * 100) : 0}%` }} />
          </div>
        ) : null}
        <p className="sr-only">{done} of {total} {total === 1 ? "lesson" : "lessons"} complete</p>
        {offline.remove.isSuccess ? (
          <p role="status" className="mt-2 text-[11px] text-muted">
            Freed {formatSize(offline.remove.data.freedBytes)}. Lessons kept offline were not touched.
          </p>
        ) : null}
        {lowDisk ? <p role="alert" className="mt-2 text-[11px] text-amber-200">Downloads are paused because less than 1 GB is free on this disk. Free up space to continue.</p> : null}
        {offline.downloads?.paused ? <p className="mt-2 text-[11px] text-amber-200">Downloads and sharing are paused. Lessons already on this computer still play.</p> : null}
        {offline.error ? <p role="alert" className="mt-2 text-[11px] text-red-300">{offline.error}</p> : null}
      </div>

      <div className="relative min-h-0 flex-1 overflow-y-auto px-1.5 py-1.5">
        {course.modules.map((module) => (
          <section key={module.id} aria-label={`Module ${module.title}`} className="mb-2">
            <div className="group/module flex items-center justify-between gap-2 px-2 pb-0.5 pt-2">
              <h3 className="truncate text-[10px] font-bold uppercase tracking-wider text-subtle">{module.title}</h3>
              {torrent ? (
                <span className="opacity-0 transition group-hover/module:opacity-100 focus-within:opacity-100">
                  <KeepOfflineToggle
                    compact
                    label={`Keep module ${module.title} offline`}
                    kept={offline.downloads?.modules.includes(module.id) ?? false}
                    inherited={offline.downloads?.course ?? false}
                    disabled={offline.change.isPending || !offline.downloads}
                    onToggle={(keep) => offline.change.mutate({ scope: { kind: "module", id: module.id }, keep })}
                  />
                </span>
              ) : null}
            </div>
            <ol>
              {module.lessons.map((lesson) => {
                const active = lesson.id === activeLessonId;
                const duration = lessonDuration(lesson);
                const video = lesson.resources.find((resource) => resource.kind === "video");
                const videoItem = video ? availability.get(video.id) : undefined;
                const removable = torrent && video && videoItem?.kept === false && videoItem.verifiedBytes > 0 && showing?.resourceId !== video.id;
                return (
                  <li
                    key={lesson.id}
                    data-active={active || undefined}
                    className={`group relative flex h-8 items-center gap-1.5 rounded-md pl-2 pr-1 transition ${active ? "bg-accent/10" : "hover:bg-white/5"}`}
                  >
                    {active ? <span aria-hidden className="absolute inset-y-1.5 left-0 w-0.5 rounded-full bg-accent" /> : null}
                    <LessonStatusIcon status={progress.statusOf(lesson.id)} />
                    <button
                      type="button"
                      onClick={() => onOpenLesson(lesson)}
                      title={lesson.title}
                      className={`min-w-0 flex-1 truncate text-left text-[13px] ${active ? "font-semibold text-fg" : "text-fg-soft hover:text-fg"}`}
                    >
                      <span className="mr-1 tabular-nums text-subtle">{number.get(lesson.id)}.</span>
                      {lesson.title}
                    </button>

                    {/* Materials: small icons; the open one is filled. */}
                    <span className="flex shrink-0 items-center">
                      {lesson.resources.map((resource) => {
                        const current = showing?.resourceId === resource.id;
                        const style = `${iconButton} ${current ? "bg-accent text-on-accent" : "text-faint hover:bg-white/10 hover:text-fg"}`;
                        if (resource.kind === "video") {
                          const item = availability.get(resource.id);
                          return (
                            <span key={resource.id} className="flex items-center">
                              {torrent ? <AvailabilityLabel compact item={item} /> : null}
                              <button type="button" onClick={() => onOpenResource(lesson, resource.id)} disabled={opening} aria-current={current ? "true" : undefined} title={current ? "Playing" : "Play video"} className={style}>
                                <Play size={12} />
                                <span className="sr-only">
                                  {current ? "Playing" : "Play video"}
                                  {torrent ? ` · ${percentOf(item)}% downloaded` : ""}
                                </span>
                              </button>
                            </span>
                          );
                        }
                        if (resource.kind === "pdf" || isReadable(resource)) {
                          const label = current ? "Reading" : resource.kind === "pdf" ? "Open PDF" : "Read";
                          return (
                            <button type="button" key={resource.id} aria-current={current ? "true" : undefined} onClick={() => onOpenResource(lesson, resource.id)} title={label} className={style}>
                              {resource.kind === "pdf" ? <File size={12} /> : <BookOpen size={12} />}
                              <span className="sr-only">{label}</span>
                            </button>
                          );
                        }
                        return null;
                      })}
                    </span>

                    {/* Duration until hover or focus, then the lesson's actions. */}
                    <span className="w-10 shrink-0 text-right text-[11px] tabular-nums text-subtle group-focus-within:hidden group-hover:hidden">
                      {duration ? formatDuration(duration) : ""}
                    </span>
                    <span className="hidden shrink-0 items-center group-focus-within:flex group-hover:flex">
                      <button
                        type="button"
                        aria-label={`Notes for ${lesson.title}`}
                        aria-pressed={active}
                        title="Notes"
                        onClick={() => onNotes(lesson)}
                        className={`${iconButton} ${active ? "text-accent-text" : "text-muted hover:bg-white/10 hover:text-fg"}`}
                      >
                        <NotebookPen size={13} />
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
                      {removable ? (
                        <button
                          type="button"
                          aria-label={`Remove download of ${lesson.title}`}
                          title="Remove download"
                          onClick={() => offline.remove.mutate([video.id])}
                          disabled={offline.remove.isPending}
                          className={`${iconButton} text-muted hover:bg-white/10 hover:text-red-300`}
                        >
                          <Eraser size={13} />
                        </button>
                      ) : null}
                    </span>
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
