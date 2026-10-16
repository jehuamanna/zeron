import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Circle, CircleDot } from "lucide-react";

import { libraryApi, type CourseProgress, type LessonStatus } from "../../services/library";

export function useCourseProgress(courseId: string | null) {
  const queryClient = useQueryClient();
  const progress = useQuery({
    queryKey: ["progress", courseId],
    queryFn: () => libraryApi.progress(courseId!),
    enabled: courseId !== null,
  });
  const mark = useMutation({
    mutationFn: ({ lessonId, completed }: { lessonId: string; completed: boolean }) =>
      libraryApi.setLessonCompleted(courseId!, lessonId, completed),
    onSuccess: (updated) => {
      queryClient.setQueryData<CourseProgress>(["progress", courseId], updated);
      void queryClient.invalidateQueries({ queryKey: ["courses"] });
    },
  });
  const statusOf = (lessonId: string): LessonStatus =>
    progress.data?.lessons.find((lesson) => lesson.lessonId === lessonId)?.status ?? "not_started";
  return { progress: progress.data, statusOf, mark };
}

export function ProgressBar({ completed, total, label }: { completed: number; total: number; label: string }) {
  const percent = total > 0 ? Math.floor((completed / total) * 100) : 0;
  return (
    <div>
      <div role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={total} aria-valuenow={completed} className="h-1.5 overflow-hidden rounded-full bg-white/10">
        <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: `${percent}%` }} />
      </div>
      <p className="mt-1 text-xs text-muted">
        {completed} of {total} {total === 1 ? "lesson" : "lessons"} complete
      </p>
    </div>
  );
}

const statusText: Record<LessonStatus, string> = {
  not_started: "Not started",
  in_progress: "In progress",
  completed: "Completed",
};

export function LessonStatusIcon({ status }: { status: LessonStatus }) {
  const icon =
    status === "completed" ? <CheckCircle2 size={15} className="text-emerald-300" /> : status === "in_progress" ? <CircleDot size={15} className="text-accent" /> : <Circle size={15} className="text-faint" />;
  return (
    <span data-lesson-status={status} title={statusText[status]} className="shrink-0">
      {icon}
      <span className="sr-only">{statusText[status]}</span>
    </span>
  );
}

export function MarkCompleteButton({ lessonTitle, completed, disabled, onToggle, compact = false, label }: {
  lessonTitle: string;
  completed: boolean;
  disabled: boolean;
  onToggle: (completed: boolean) => void;
  /** Icon only, for dense lesson lists. */
  compact?: boolean;
  /** Overrides the accessible name. */
  label?: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={completed}
      aria-label={label ?? `Mark ${lessonTitle} complete`}
      title={completed ? "Completed. Click to mark not done" : "Mark complete"}
      disabled={disabled}
      onClick={() => onToggle(!completed)}
      className={
        compact
          ? `grid size-6 place-items-center rounded-md disabled:opacity-60 ${completed ? "text-emerald-300" : "text-faint hover:bg-white/10 hover:text-fg"}`
          : `inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium disabled:opacity-60 ${completed ? "bg-emerald-300/15 text-emerald-200" : "bg-white/5 text-fg-soft hover:bg-white/10"}`
      }
    >
      <CheckCircle2 size={compact ? 14 : 13} />
      {compact ? null : completed ? "Completed" : "Mark complete"}
    </button>
  );
}
