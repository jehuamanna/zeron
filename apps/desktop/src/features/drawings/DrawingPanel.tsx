import { useQuery, useQueryClient } from "@tanstack/react-query";
import { PenTool } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi } from "../../services/library";
import type { SceneChange } from "./DrawingCanvas";

const DrawingCanvas = lazy(() => import("./DrawingCanvas"));

/** How long drawing pauses before it is saved. */
export const DRAWING_SAVE_DELAY_MS = 800;

type Status = "idle" | "saving" | "saved" | "conflict" | "failed";

/** A lesson's Excalidraw drawing, saved as the learner draws. */
export function DrawingPanel({ courseId, lessonId, lessonTitle, fill = false }: {
  courseId: string;
  lessonId: string;
  lessonTitle: string;
  /** Open at once and fill the parent, as in the workspace's Sketch tab. */
  fill?: boolean;
}) {
  const [open, setOpen] = useState(false);
  if (fill) {
    return (
      <section aria-label={`Drawing for ${lessonTitle}`} className="flex h-full min-h-0 flex-col">
        <DrawingEditor courseId={courseId} lessonId={lessonId} fill />
      </section>
    );
  }
  return (
    <section aria-label={`Drawing for ${lessonTitle}`} className="mt-6 rounded-3xl border border-white/10 bg-white/[0.03] p-5">
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-semibold">Drawing</h2>
        <Button variant="ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
          <PenTool size={15} /> {open ? "Close drawing" : "Open drawing"}
        </Button>
      </div>
      {open ? <DrawingEditor courseId={courseId} lessonId={lessonId} /> : null}
    </section>
  );
}

function DrawingEditor({ courseId, lessonId, fill = false }: { courseId: string; lessonId: string; fill?: boolean }) {
  const queryClient = useQueryClient();
  const queryKey = ["drawing", courseId, lessonId];
  const drawing = useQuery({ queryKey, queryFn: () => libraryApi.drawing(courseId, lessonId), staleTime: Infinity, gcTime: 0 });
  const [status, setStatus] = useState<Status>("idle");
  const revision = useRef<number | null>(null);
  const savedScene = useRef<string | null>(null);
  const sentFiles = useRef(new Set<string>());
  const pending = useRef<SceneChange | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saving = useRef(false);
  const conflicted = useRef(false);
  // Excalidraw reports the scene it restored as a change; that is the baseline.
  const restored = useRef(false);

  useEffect(() => {
    if (!drawing.data && drawing.data !== null) return;
    revision.current = drawing.data?.revision ?? null;
    savedScene.current = drawing.data?.sceneJson ?? null;
    sentFiles.current = new Set(drawing.data?.files.map((file) => file.id));
    conflicted.current = false;
    restored.current = false;
  }, [drawing.data]);

  async function flush() {
    if (saving.current || conflicted.current || !pending.current) return;
    const change = pending.current;
    pending.current = null;
    saving.current = true;
    setStatus("saving");
    const files = change.files.filter((file) => !sentFiles.current.has(file.id));
    try {
      const result = await libraryApi.saveDrawing(courseId, lessonId, revision.current, change.sceneJson, files);
      if (result.outcome === "saved") {
        revision.current = result.revision;
        savedScene.current = change.sceneJson;
        for (const file of files) sentFiles.current.add(file.id);
        setStatus("saved");
      } else {
        conflicted.current = true;
        pending.current = null;
        setStatus("conflict");
      }
    } catch (error) {
      pending.current ??= change;
      setStatus("failed");
      console.warn(asCommandError(error)?.message ?? error);
    } finally {
      saving.current = false;
    }
    if (pending.current && !conflicted.current) void flush();
  }

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
      void flush();
    },
    // Save whatever is pending when the editor closes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  function changed(change: SceneChange) {
    if (!restored.current) {
      restored.current = true;
      if (savedScene.current !== null && change.files.every((file) => sentFiles.current.has(file.id))) {
        savedScene.current = change.sceneJson;
        return;
      }
    }
    if (change.sceneJson === savedScene.current && change.files.every((file) => sentFiles.current.has(file.id))) return;
    if (savedScene.current === null && revision.current === null && isEmpty(change.sceneJson)) return;
    pending.current = change;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void flush(), DRAWING_SAVE_DELAY_MS);
  }

  if (drawing.isPending) return <p className={`${fill ? "p-4" : "mt-3"} text-sm text-muted`}>Opening the drawing…</p>;
  if (drawing.isError) {
    return <p role="alert" className={`${fill ? "p-4" : "mt-3"} text-sm text-red-300`}>{asCommandError(drawing.error)?.message ?? "This drawing could not be opened."}</p>;
  }
  return (
    <div className={fill ? "relative flex min-h-0 flex-1 flex-col" : "mt-3"}>
      <p aria-live="polite" className={fill ? "pointer-events-none absolute bottom-2 right-3 z-10 text-[11px] text-subtle" : "mb-2 min-h-4 text-xs text-slate-400"}>
        {status === "saving" ? "Saving…" : status === "saved" ? "Saved" : status === "failed" ? "The drawing could not be saved; it will be retried as you draw." : ""}
      </p>
      {status === "conflict" ? (
        <p role="alert" className={`${fill ? "mx-3 mt-2" : ""} mb-2 text-sm text-amber-200`}>
          This drawing was changed in another window, so your latest strokes were not saved over it.{" "}
          <button
            type="button"
            className="underline"
            onClick={() => {
              setStatus("idle");
              void queryClient.invalidateQueries({ queryKey });
            }}
          >
            Load the saved drawing
          </button>
        </p>
      ) : null}
      <Suspense fallback={<p className="p-4 text-sm text-muted">Loading the drawing tools…</p>}>
        <DrawingCanvas
          fill={fill}
          key={drawing.data?.revision ?? "new"}
          sceneJson={drawing.data?.sceneJson ?? null}
          files={drawing.data?.files ?? []}
          onChange={changed}
        />
      </Suspense>
    </div>
  );
}

function isEmpty(sceneJson: string) {
  try {
    return (JSON.parse(sceneJson) as { elements?: unknown[] }).elements?.length === 0;
  } catch {
    return false;
  }
}
