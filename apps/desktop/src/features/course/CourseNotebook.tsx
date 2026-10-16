import { useQuery } from "@tanstack/react-query";
import { Clock, FileText, NotebookPen, PenTool, Search } from "lucide-react";
import { lazy, Suspense, useMemo, useState } from "react";

import { anchorLabel } from "../notes/NotesPanel";
import { NoteEditor } from "../notes/NoteEditor";
import { asCommandError, libraryApi, type Note } from "../../services/library";
import type { Lesson, PlacedLesson } from "./lessons";

// Excalidraw renders the previews; it loads only when a sketch is shown.
const SketchPreview = lazy(() => import("../drawings/SketchPreview"));

type Show = "all" | "notes" | "sketches";

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

/** Every note and sketch of the course in one place, in lesson order. */
export function CourseNotebook({ courseId, placed, onOpenLesson, onSeek, onOpenPage, onOpenSketch, onEditNotes }: {
  courseId: string;
  placed: PlacedLesson[];
  onOpenLesson: (lesson: Lesson) => void;
  onSeek: (resourceId: string, seconds: number) => void;
  onOpenPage: (resourceId: string, page: number) => void;
  onOpenSketch: (lesson: Lesson) => void;
  onEditNotes: (lesson: Lesson) => void;
}) {
  const notes = useQuery({ queryKey: ["notes", courseId, null], queryFn: () => libraryApi.notes(courseId, null), staleTime: 0 });
  const drawn = useQuery({ queryKey: ["drawn-lessons", courseId], queryFn: () => libraryApi.drawnLessons(courseId), staleTime: 0 });
  const [show, setShow] = useState<Show>("all");
  const [filter, setFilter] = useState("");
  const query = filter.trim().toLowerCase();

  const groups = useMemo(() => {
    const byLesson = new Map<string, Note[]>();
    for (const note of notes.data ?? []) {
      if (query && !note.text.toLowerCase().includes(query)) continue;
      byLesson.set(note.lessonId, [...(byLesson.get(note.lessonId) ?? []), note]);
    }
    const sketches = new Set(drawn.data ?? []);
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
  }, [notes.data, drawn.data, placed, show, query]);

  const noteCount = notes.data?.length ?? 0;
  const sketchCount = (drawn.data ?? []).filter((lessonId) => placed.some((item) => item.lesson.id === lessonId)).length;
  const failed = notes.isError ? notes.error : drawn.isError ? drawn.error : null;

  return (
    <div className="relative h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8">
        <p className="mb-1 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Notebook</p>
        <h1 className="font-display text-3xl font-semibold tracking-tight">Everything you captured</h1>
        <p className="mt-1 text-sm text-muted">
          {noteCount} {noteCount === 1 ? "note" : "notes"} · {sketchCount} {sketchCount === 1 ? "sketch" : "sketches"}, in lesson order.
        </p>

        <div className="sticky top-0 z-10 -mx-2 mt-5 flex flex-wrap items-center gap-2 bg-bg/95 px-2 py-2 backdrop-blur">
          <div role="group" aria-label="Show" className="flex items-center gap-0.5 rounded-lg bg-sunken p-0.5">
            {(["all", "notes", "sketches"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={show === value}
                onClick={() => setShow(value)}
                className={`rounded-md px-3 py-1 text-xs font-medium capitalize ${show === value ? "bg-surface text-fg shadow-sm" : "text-muted hover:text-fg"}`}
              >
                {value}
              </button>
            ))}
          </div>
          <label className="ml-auto flex w-full items-center gap-2 rounded-lg border border-line bg-surface px-2.5 py-1.5 text-muted sm:w-64">
            <Search size={14} />
            <input
              type="search"
              aria-label="Search notes"
              placeholder="Search notes"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              className="w-full bg-transparent text-sm text-fg outline-none placeholder:text-faint"
            />
          </label>
        </div>

        {failed ? (
          <p role="alert" className="mt-6 text-sm text-red-300">{asCommandError(failed)?.message ?? "The notebook could not be read."}</p>
        ) : notes.isPending || drawn.isPending ? (
          <p className="mt-6 text-sm text-muted">Opening the notebook…</p>
        ) : groups.length === 0 ? (
          <div className="mt-10 grid place-items-center text-center">
            <NotebookPen size={26} className="mb-2 text-faint" />
            <p className="text-sm text-muted">{query ? `No note mentions “${filter.trim()}”.` : "Nothing here yet. Notes and sketches you make in lessons collect here."}</p>
          </div>
        ) : (
          <ol aria-label="Lessons with notes" className="mt-4 space-y-8">
            {groups.map(({ item, notes: lessonNotes, sketch }) => (
              <li key={item.lesson.id}>
                <div className="mb-3 flex items-baseline justify-between gap-3 border-b border-line pb-2">
                  <div className="min-w-0">
                    <p className="text-[11px] uppercase tracking-wider text-subtle">{item.moduleTitle} · Lesson {item.index + 1}</p>
                    <h2 className="truncate font-display text-lg font-semibold">
                      <button type="button" className="hover:text-accent-text" onClick={() => onOpenLesson(item.lesson)}>{item.lesson.title}</button>
                    </h2>
                  </div>
                  <button type="button" className="shrink-0 text-xs text-muted hover:text-fg" onClick={() => onEditNotes(item.lesson)}>
                    Edit notes
                  </button>
                </div>
                <ul aria-label={`Notes in ${item.lesson.title}`} className="space-y-3">
                  {lessonNotes.map((note) => (
                    <li key={note.noteId} className="rounded-xl border border-line bg-surface p-4 shadow-sm">
                      {note.anchor.kind === "lesson" ? (
                        <span className="mb-2 inline-block rounded-md bg-white/5 px-2 py-0.5 text-[11px] font-medium text-muted">Lesson</span>
                      ) : (
                        <button
                          type="button"
                          aria-label={`Go to ${anchorLabel(note.anchor)} in ${item.lesson.title}`}
                          onClick={() => {
                            const anchor = note.anchor;
                            if (anchor.kind === "videoTime") onSeek(anchor.resourceId, anchor.seconds);
                            if (anchor.kind === "pdfPage") onOpenPage(anchor.resourceId, anchor.page);
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
                        <button type="button" className="text-xs text-muted hover:text-fg" onClick={() => onOpenSketch(item.lesson)}>Open sketch</button>
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
        )}
      </div>
    </div>
  );
}
