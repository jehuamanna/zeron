import { useQuery } from "@tanstack/react-query";
import { NotebookPen } from "lucide-react";
import { useMemo, useState } from "react";

import { buildGroups, countSketches, LessonGroups, NotebookFilters, type NotebookActions, type Show } from "../notebook/LessonGroups";
import { asCommandError, libraryApi } from "../../services/library";
import type { PlacedLesson } from "./lessons";

/** Every note and sketch of the course in one place, in lesson order. */
export function CourseNotebook({ courseId, placed, ...actions }: NotebookActions & { courseId: string; placed: PlacedLesson[] }) {
  const notes = useQuery({ queryKey: ["notes", courseId, null], queryFn: () => libraryApi.notes(courseId, null), staleTime: 0 });
  const drawn = useQuery({ queryKey: ["drawn-lessons", courseId], queryFn: () => libraryApi.drawnLessons(courseId), staleTime: 0 });
  const [show, setShow] = useState<Show>("all");
  const [filter, setFilter] = useState("");
  const query = filter.trim().toLowerCase();
  const groups = useMemo(() => buildGroups(placed, notes.data ?? [], drawn.data ?? [], show, query), [placed, notes.data, drawn.data, show, query]);

  const noteCount = notes.data?.length ?? 0;
  const sketchCount = countSketches(placed, drawn.data ?? []);
  const failed = notes.isError ? notes.error : drawn.isError ? drawn.error : null;

  return (
    <div className="relative h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8">
        <p className="mb-1 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Notebook</p>
        <h1 className="font-display text-3xl font-bold tracking-tight">Everything you captured</h1>
        <p className="mt-1 text-sm text-muted">
          {noteCount} {noteCount === 1 ? "note" : "notes"} · {sketchCount} {sketchCount === 1 ? "sketch" : "sketches"}, in lesson order.
        </p>
        <NotebookFilters show={show} onShow={setShow} filter={filter} onFilter={setFilter} />

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
          <div className="mt-4">
            <LessonGroups courseId={courseId} groups={groups} actions={actions} />
          </div>
        )}
      </div>
    </div>
  );
}
