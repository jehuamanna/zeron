import { useQueries } from "@tanstack/react-query";
import { ArrowRight, ChevronLeft, NotebookPen } from "lucide-react";
import { useState } from "react";

import { placeLessons } from "../course/lessons";
import type { WorkspaceTarget } from "../course/target";
import { asCommandError, libraryApi, type CourseSummary } from "../../services/library";
import { buildGroups, countSketches, LessonGroups, NotebookFilters, type Show } from "./LessonGroups";

/** Every note and sketch across the library, course by course, in lesson order. */
export function LibraryNotebook({ courses, onOpen, onBack }: {
  courses: CourseSummary[];
  /** Opens a course, optionally at a note's moment, page, or lesson. */
  onOpen: (courseId: string, target: WorkspaceTarget | null) => void;
  onBack: () => void;
}) {
  const [show, setShow] = useState<Show>("all");
  const [filter, setFilter] = useState("");
  const [only, setOnly] = useState<string>("");
  const query = filter.trim().toLowerCase();

  const details = useQueries({ queries: courses.map((course) => ({ queryKey: ["course-detail", course.courseId], queryFn: () => libraryApi.detail(course.courseId) })) });
  const notes = useQueries({ queries: courses.map((course) => ({ queryKey: ["notes", course.courseId, null], queryFn: () => libraryApi.notes(course.courseId, null), staleTime: 0 })) });
  const drawn = useQueries({ queries: courses.map((course) => ({ queryKey: ["drawn-lessons", course.courseId], queryFn: () => libraryApi.drawnLessons(course.courseId), staleTime: 0 })) });

  const sections = courses.map((course, index) => {
    const detail = details[index]?.data;
    const placed = detail ? placeLessons(detail) : [];
    const courseNotes = notes[index]?.data ?? [];
    const courseDrawn = drawn[index]?.data ?? [];
    return {
      course,
      groups: buildGroups(placed, courseNotes, courseDrawn, show, query),
      noteCount: courseNotes.length,
      sketchCount: countSketches(placed, courseDrawn),
    };
  });

  const pending = [...details, ...notes, ...drawn].some((item) => item.isPending);
  const failed = [...details, ...notes, ...drawn].find((item) => item.isError)?.error ?? null;
  const shown = sections.filter((section) => (!only || section.course.courseId === only) && section.groups.length > 0);
  const totalNotes = sections.reduce((sum, section) => sum + section.noteCount, 0);
  const totalSketches = sections.reduce((sum, section) => sum + section.sketchCount, 0);
  const withSomething = sections.filter((section) => section.noteCount + section.sketchCount > 0);

  return (
    <div className="mx-auto max-w-3xl">
      <button type="button" onClick={onBack} className="mb-4 inline-flex items-center gap-1 rounded-md px-1 py-1 text-sm text-muted hover:text-fg">
        <ChevronLeft size={16} /> Courses
      </button>
      <p className="mb-1 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Notebook</p>
      <h1 className="font-display text-3xl font-semibold tracking-tight sm:text-4xl">All your notes</h1>
      <p className="mt-1 text-sm text-muted">
        {totalNotes} {totalNotes === 1 ? "note" : "notes"} · {totalSketches} {totalSketches === 1 ? "sketch" : "sketches"} across {withSomething.length} {withSomething.length === 1 ? "course" : "courses"}.
      </p>

      <NotebookFilters show={show} onShow={setShow} filter={filter} onFilter={setFilter}>
        <select
          aria-label="Course"
          value={only}
          onChange={(event) => setOnly(event.target.value)}
          className="max-w-56 truncate rounded-lg border border-line bg-surface px-2 py-1.5 text-xs text-fg"
        >
          <option value="">All courses</option>
          {withSomething.map(({ course }) => <option key={course.courseId} value={course.courseId}>{course.title}</option>)}
        </select>
      </NotebookFilters>

      {failed ? (
        <p role="alert" className="mt-6 text-sm text-red-300">{asCommandError(failed)?.message ?? "The notebook could not be read."}</p>
      ) : pending ? (
        <p className="mt-6 text-sm text-muted">Opening the notebook…</p>
      ) : shown.length === 0 ? (
        <div className="mt-12 grid place-items-center text-center">
          <NotebookPen size={26} className="mb-2 text-faint" />
          <p className="text-sm text-muted">{query ? `No note mentions “${filter.trim()}”.` : "Nothing here yet. Notes and sketches you make in lessons collect here."}</p>
        </div>
      ) : (
        <div className="mt-4 space-y-12">
          {shown.map(({ course, groups }) => {
            const open = (target: WorkspaceTarget | null) => onOpen(course.courseId, target);
            const courseId = course.courseId;
            return (
              <section key={courseId} aria-label={course.title}>
                <div className="mb-5 flex items-end justify-between gap-3 border-b-2 border-accent/30 pb-2">
                  <div className="min-w-0">
                    <h2 className="truncate font-display text-2xl font-semibold">{course.title}</h2>
                    <p className="text-xs text-subtle">{course.publisherName}</p>
                  </div>
                  <button type="button" onClick={() => open(null)} className="inline-flex shrink-0 items-center gap-1 text-xs font-medium text-accent-text hover:underline">
                    Open course <ArrowRight size={13} />
                  </button>
                </div>
                <LessonGroups
                  courseId={courseId}
                  groups={groups}
                  headingLevel={3}
                  label={`Lessons with notes in ${course.title}`}
                  actions={{
                    onOpenLesson: (lesson) => open({ kind: "lesson", courseId, lessonId: lesson.id, tab: "notes" }),
                    onEditNotes: (lesson) => open({ kind: "lesson", courseId, lessonId: lesson.id, tab: "notes" }),
                    onOpenSketch: (lesson) => open({ kind: "lesson", courseId, lessonId: lesson.id, tab: "sketch" }),
                    onSeek: (resourceId, seconds) => open({ kind: "moment", courseId, resourceId, seconds }),
                    onOpenPage: (resourceId, page) => open({ kind: "page", courseId, resourceId, page }),
                  }}
                />
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
