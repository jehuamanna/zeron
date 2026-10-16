import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, GitMerge } from "lucide-react";
import { useState } from "react";

import { asCommandError, libraryApi, type AnnotationMove, type AnnotationMoveReport, type CourseDetail } from "../../services/library";

const reasons: Record<NonNullable<AnnotationMove["reason"]>, string> = {
  same_lesson: "Same lesson; some of its files changed",
  same_content: "Has the same file",
  same_title: "Has the same title",
};

function count(value: number, word: string) {
  return `${value} ${word}${value === 1 ? "" : "s"}`;
}

/** What a lesson's annotations are, in words. */
export function describeAnnotations(item: AnnotationMove): string {
  const parts = [];
  if (item.notes) parts.push(count(item.notes, "note"));
  if (item.hasDrawing) parts.push("a drawing");
  if (item.hasProgress) parts.push("progress");
  return parts.join(", ");
}

/** What a move did, in words. */
export function describeReport(report: AnnotationMoveReport): string {
  const parts = [`Moved ${count(report.notesMoved, "note")}`];
  if (report.drawingMoved) parts.push("the drawing");
  if (report.progressMoved) parts.push("progress");
  let text = parts.join(", ") + ".";
  if (report.notesWidened) text += ` ${count(report.notesWidened, "note")} now ${report.notesWidened === 1 ? "points" : "point"} at the whole lesson because ${report.notesWidened === 1 ? "its" : "their"} file changed.`;
  if (report.drawingKept) text += " The drawing stayed in its old place because the lesson already has one.";
  return text;
}

/**
 * After a course is updated, lists lessons whose notes, drawing, or progress
 * the new version no longer shows, and moves them where the learner chooses.
 */
export function AnnotationMovesCard({ course }: { course: CourseDetail }) {
  const moves = useQuery({ queryKey: ["annotation-moves", course.courseId], queryFn: () => libraryApi.annotationMoves(course.courseId) });
  const [report, setReport] = useState<string | null>(null);
  if (!moves.data?.length && !report) return null;
  return (
    <section aria-labelledby="annotation-moves-heading" className="mt-5 max-w-2xl rounded-2xl border border-cyan-300/20 bg-cyan-300/5 px-4 py-3 text-sm">
      <h2 id="annotation-moves-heading" className="flex items-center gap-2 font-semibold text-cyan-100"><GitMerge size={16} /> Your notes after the update</h2>
      {moves.data?.length ? (
        <>
          <p className="mt-1 text-slate-300">
            Version {course.courseVersion} changed some lessons, so these notes are hidden. Choose where each lesson's notes belong now; nothing moves until you do.
          </p>
          <ul aria-label="Annotations to move" className="mt-3 space-y-3">
            {moves.data.map((item) => <MoveRow key={item.fromLessonId} item={item} course={course} onMoved={setReport} />)}
          </ul>
        </>
      ) : null}
      {report ? <p role="status" className="mt-2 text-xs text-emerald-300">{report}</p> : null}
    </section>
  );
}

function MoveRow({ item, course, onMoved }: { item: AnnotationMove; course: CourseDetail; onMoved: (report: string) => void }) {
  const queryClient = useQueryClient();
  const lessons = course.modules.flatMap((module) => module.lessons.map((lesson) => ({ ...lesson, module: module.title })));
  const [target, setTarget] = useState(item.suggestedLessonId ?? "");
  const move = useMutation({
    mutationFn: () => libraryApi.moveAnnotations(course.courseId, item.fromLessonId, target),
    onSuccess: (report) => {
      onMoved(describeReport(report));
      for (const key of [["annotation-moves", course.courseId], ["notes", course.courseId], ["drawing", course.courseId], ["progress", course.courseId]]) {
        void queryClient.invalidateQueries({ queryKey: key });
      }
    },
  });
  const selectId = `move-target-${item.fromLessonId}`;
  const name = item.fromTitle ? `“${item.fromTitle}”` : "A lesson";
  return (
    <li className="rounded-xl border border-white/10 bg-black/20 p-3">
      <p className="text-slate-200">
        {name}
        {item.fromVersion && item.reason !== "same_lesson" ? <span className="text-slate-500"> from version {item.fromVersion}</span> : null}
        <span className="text-slate-400"> · {describeAnnotations(item)}</span>
      </p>
      {item.reason && item.suggestedTitle ? (
        <p className="mt-1 text-xs text-slate-400">Suggested: “{item.suggestedTitle}”. {reasons[item.reason]}.{item.notesWidened ? ` ${count(item.notesWidened, "note")} will point at the whole lesson because ${item.notesWidened === 1 ? "its" : "their"} file changed.` : ""}</p>
      ) : (
        <p className="mt-1 text-xs text-slate-400">No lesson in this version matches. Choose one, or leave the notes hidden.</p>
      )}
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <label htmlFor={selectId} className="sr-only">Move {name} to</label>
        <select id={selectId} value={target} onChange={(event) => setTarget(event.target.value)} className="min-w-0 max-w-full rounded-lg border border-white/10 bg-black/40 px-2 py-1 text-xs text-slate-100">
          <option value="">Choose a lesson…</option>
          {lessons.map((lesson) => <option key={lesson.id} value={lesson.id}>{lesson.module} › {lesson.title}</option>)}
        </select>
        <button type="button" onClick={() => move.mutate()} disabled={!target || move.isPending} className="flex items-center gap-1 rounded-lg bg-cyan-300 px-2.5 py-1 text-xs font-semibold text-slate-950 disabled:opacity-40">
          <ArrowRight size={13} /> {move.isPending ? "Moving…" : "Move here"}
        </button>
      </div>
      {move.isError ? <p role="alert" className="mt-1 text-xs text-red-300">{asCommandError(move.error)?.message ?? "The notes could not be moved."}</p> : null}
    </li>
  );
}
