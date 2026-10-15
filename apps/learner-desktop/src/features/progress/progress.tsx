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
        <div className="h-full rounded-full bg-emerald-300" style={{ width: `${percent}%` }} />
      </div>
      <p className="mt-1 text-xs text-slate-400">
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
    status === "completed" ? <CheckCircle2 size={15} className="text-emerald-300" /> : status === "in_progress" ? <CircleDot size={15} className="text-cyan-200" /> : <Circle size={15} className="text-slate-600" />;
  return (
    <span data-lesson-status={status} title={statusText[status]} className="mt-0.5 shrink-0">
      {icon}
      <span className="sr-only">{statusText[status]}</span>
    </span>
  );
}

export function MarkCompleteButton({ lessonTitle, completed, disabled, onToggle }: { lessonTitle: string; completed: boolean; disabled: boolean; onToggle: (completed: boolean) => void }) {
  return (
    <button
      type="button"
      aria-pressed={completed}
      aria-label={`Mark ${lessonTitle} complete`}
      disabled={disabled}
      onClick={() => onToggle(!completed)}
      className={`rounded-md px-2 py-1 text-[11px] disabled:opacity-60 ${completed ? "bg-emerald-300/15 text-emerald-200" : "bg-white/5 text-slate-300 hover:bg-white/10"}`}
    >
      {completed ? "Completed" : "Mark complete"}
    </button>
  );
}
