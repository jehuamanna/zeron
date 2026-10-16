import { Clock, FileText, PenTool, Search } from "lucide-react";
import { lazy, Suspense } from "react";

import type { Lesson, PlacedLesson } from "../course/lessons";
import { NoteEditor } from "../notes/NoteEditor";
import { anchorLabel } from "../notes/NotesPanel";
import type { Note } from "../../services/library";

// Excalidraw renders the previews; it loads only when a sketch is shown.
const SketchPreview = lazy(() => import("../drawings/SketchPreview"));

export type Show = "all" | "notes" | "sketches";

/** One lesson's notes, in reading order, and whether it has a sketch to show. */
export interface LessonGroup {
  item: PlacedLesson;
  notes: Note[];
  sketch: boolean;
}

/** Order inside a lesson: whole-lesson notes, then by moment, then by page. */
function anchorOrder(note: Note): [number, number] {
  switch (note.anchor.kind) {
    case "lesson":
      return [0, note.createdAtUnixSeconds];
    case "videoTime":
      return [1, note.anchor.seconds];
    case "pdfPage":
      return [2, note.anchor.page];
  }
}

/** Groups a course's notes and sketches by lesson in course order, applying the filters. */
export function buildGroups(placed: PlacedLesson[], notes: Note[], drawnLessons: string[], show: Show, query: string): LessonGroup[] {
  const byLesson = new Map<string, Note[]>();
  for (const note of notes) {
    if (query && !note.text.toLowerCase().includes(query)) continue;
    byLesson.set(note.lessonId, [...(byLesson.get(note.lessonId) ?? []), note]);
  }
  const sketches = new Set(drawnLessons);
  return placed
    .map((item) => ({
      item,
      notes: show === "sketches" ? [] : (byLesson.get(item.lesson.id) ?? []).sort((a, b) => {
        const [kindA, atA] = anchorOrder(a);
        const [kindB, atB] = anchorOrder(b);
        return kindA - kindB || atA - atB;
      }),
      sketch: show !== "notes" && !query && sketches.has(item.lesson.id),
    }))
    .filter((group) => group.notes.length > 0 || group.sketch);
}

/** Sketches whose lesson is still in the course, for counts. */
export function countSketches(placed: PlacedLesson[], drawnLessons: string[]) {
  return drawnLessons.filter((lessonId) => placed.some((item) => item.lesson.id === lessonId)).length;
}

/** What a notebook entry can open. */
export interface NotebookActions {
  onOpenLesson: (lesson: Lesson) => void;
  onSeek: (resourceId: string, seconds: number) => void;
  onOpenPage: (resourceId: string, page: number) => void;
  onOpenSketch: (lesson: Lesson) => void;
  onEditNotes: (lesson: Lesson) => void;
}

/** The All / Notes / Sketches switch and the note search. */
export function NotebookFilters({ show, onShow, filter, onFilter, children }: {
  show: Show;
  onShow: (show: Show) => void;
  filter: string;
  onFilter: (filter: string) => void;
  /** Extra controls, such as a course picker. */
  children?: React.ReactNode;
}) {
  return (
    <div className="sticky top-0 z-10 -mx-2 mt-5 flex flex-wrap items-center gap-2 bg-bg/95 px-2 py-2 backdrop-blur">
      <div role="group" aria-label="Show" className="flex items-center gap-0.5 rounded-lg bg-sunken p-0.5">
        {(["all", "notes", "sketches"] as const).map((value) => (
          <button
            key={value}
            type="button"
            aria-pressed={show === value}
            onClick={() => onShow(value)}
            className={`rounded-md px-3 py-1 text-xs font-medium capitalize ${show === value ? "bg-surface text-fg shadow-sm" : "text-muted hover:text-fg"}`}
          >
            {value}
          </button>
        ))}
      </div>
      {children}
      <label className="ml-auto flex w-full items-center gap-2 rounded-lg border border-line bg-surface px-2.5 py-1.5 text-muted sm:w-64">
        <Search size={14} />
        <input
          type="search"
          aria-label="Search notes"
          placeholder="Search notes"
          value={filter}
          onChange={(event) => onFilter(event.target.value)}
          className="w-full bg-transparent text-sm text-fg outline-none placeholder:text-faint"
        />
      </label>
    </div>
  );
}

/** Lessons with their notes and sketch previews. */
export function LessonGroups({ courseId, groups, actions, label = "Lessons with notes", headingLevel = 2 }: {
  courseId: string;
  groups: LessonGroup[];
  actions: NotebookActions;
  label?: string;
  /** 2 on a course's notebook; 3 under a course heading in the library notebook. */
  headingLevel?: 2 | 3;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return (
    <ol aria-label={label} className="space-y-8">
      {groups.map(({ item, notes, sketch }) => (
        <li key={item.lesson.id}>
          <div className="mb-3 flex items-baseline justify-between gap-3 border-b border-line pb-2">
            <div className="min-w-0">
              <p className="text-[11px] uppercase tracking-wider text-subtle">{item.moduleTitle} · Lesson {item.index + 1}</p>
              <Heading className="truncate font-display text-lg font-semibold">
                <button type="button" className="hover:text-accent-text" onClick={() => actions.onOpenLesson(item.lesson)}>{item.lesson.title}</button>
              </Heading>
            </div>
            <button type="button" className="shrink-0 text-xs text-muted hover:text-fg" onClick={() => actions.onEditNotes(item.lesson)}>
              Edit notes
            </button>
          </div>
          <ul aria-label={`Notes in ${item.lesson.title}`} className="space-y-3">
            {notes.map((note) => (
              <li key={note.noteId} className="rounded-xl border border-line bg-surface p-4 shadow-sm">
                {note.anchor.kind === "lesson" ? (
                  <span className="mb-2 inline-block rounded-md bg-white/5 px-2 py-0.5 text-[11px] font-medium text-muted">Lesson</span>
                ) : (
                  <button
                    type="button"
                    aria-label={`Go to ${anchorLabel(note.anchor)} in ${item.lesson.title}`}
                    onClick={() => {
                      const anchor = note.anchor;
                      if (anchor.kind === "videoTime") actions.onSeek(anchor.resourceId, anchor.seconds);
                      if (anchor.kind === "pdfPage") actions.onOpenPage(anchor.resourceId, anchor.page);
                    }}
                    className="mb-2 inline-flex items-center gap-1 rounded-md bg-accent/10 px-2 py-0.5 text-[11px] font-semibold text-accent-text hover:bg-accent/20"
                  >
                    {note.anchor.kind === "videoTime" ? <Clock size={11} /> : <FileText size={11} />}
                    {anchorLabel(note.anchor)}
                  </button>
                )}
                <NoteEditor key={`${note.noteId}:${note.revision}`} body={note.body} editable={false} label="Note" />
              </li>
            ))}
            {sketch ? (
              <li className="rounded-xl border border-line bg-surface p-3 shadow-sm">
                <div className="mb-2 flex items-center justify-between">
                  <span className="inline-flex items-center gap-1 text-[11px] font-medium text-muted"><PenTool size={11} /> Sketch</span>
                  <button type="button" className="text-xs text-muted hover:text-fg" onClick={() => actions.onOpenSketch(item.lesson)}>Open sketch</button>
                </div>
                <Suspense fallback={<div aria-hidden className="h-40 animate-pulse rounded-lg bg-white/5" />}>
                  <SketchPreview courseId={courseId} lessonId={item.lesson.id} title={item.lesson.title} />
                </Suspense>
              </li>
            ) : null}
          </ul>
        </li>
      ))}
    </ol>
  );
}
