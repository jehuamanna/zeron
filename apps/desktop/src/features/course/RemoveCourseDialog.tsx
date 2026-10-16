import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { useEffect } from "react";

import { Button } from "../../components/ui/button";
import { formatSize } from "../downloads/downloads";
import { asCommandError, libraryApi, type CourseSource } from "../../services/library";

/** Confirms removing a course, and says exactly what goes and what stays. */
export function RemoveCourseDialog({ course, onClose, onRemoved }: {
  course: { courseId: string; title: string; source: CourseSource; totalResourceBytes: number };
  onClose: () => void;
  onRemoved: () => void;
}) {
  const queryClient = useQueryClient();
  const remove = useMutation({
    mutationFn: () => libraryApi.removeCourse(course.courseId),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["courses"] }),
        queryClient.invalidateQueries({ queryKey: ["removed-courses"] }),
        queryClient.invalidateQueries({ queryKey: ["search"] }),
      ]);
      queryClient.removeQueries({ queryKey: ["course-detail", course.courseId] });
      onRemoved();
    },
  });
  useEffect(() => {
    const close = (event: KeyboardEvent) => event.key === "Escape" && !remove.isPending && onClose();
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [onClose, remove.isPending]);

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-[#000]/40 p-4 backdrop-blur-sm" onMouseDown={(event) => event.target === event.currentTarget && !remove.isPending && onClose()}>
      <div role="alertdialog" aria-modal="true" aria-labelledby="remove-course-title" aria-describedby="remove-course-what" className="w-full max-w-md rounded-2xl border border-line bg-surface p-5 shadow-panel">
        <div className="flex items-start gap-3">
          <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-red-400/10 text-red-300"><Trash2 size={17} /></span>
          <div className="min-w-0">
            <h2 id="remove-course-title" className="font-display text-lg font-bold">Remove “{course.title}”?</h2>
            <div id="remove-course-what" className="mt-2 space-y-2 text-sm text-muted">
              <p>
                {course.source === "torrent"
                  ? `The course leaves your library, its download stops, and the ${formatSize(course.totalResourceBytes)} it may have downloaded is deleted from this computer.`
                  : course.source === "youtube"
                    ? "The video leaves your library. Zeron never downloaded it, so nothing is deleted from this computer."
                    : "The course leaves your library. The folder you imported it from is not touched."}
              </p>
              <p className="text-fg-soft">Your notes, highlights, sketches, and progress stay. They come back if you import this course again, and the notebook still shows them.</p>
            </div>
          </div>
        </div>
        {remove.isError ? <p role="alert" className="mt-3 text-sm text-red-300">{asCommandError(remove.error)?.message ?? "The course could not be removed."}</p> : null}
        <div className="mt-5 flex justify-end gap-2">
          <Button autoFocus variant="ghost" disabled={remove.isPending} onClick={onClose}>Cancel</Button>
          <Button className="bg-red-400 text-[#fff] hover:bg-red-400/90" disabled={remove.isPending} onClick={() => remove.mutate()}>
            {remove.isPending ? "Removing…" : "Remove course"}
          </Button>
        </div>
      </div>
    </div>
  );
}
