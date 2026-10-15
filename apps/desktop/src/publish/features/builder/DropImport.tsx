import { FolderInput, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { asCommandError, publisherApi, type DraftDetail, type DropOutcome, type DropSummary, type SkipReason } from "../../services/publisher";
import type { DraftWriter } from "./draftWriter";

const reasons: Record<SkipReason, string> = {
  hidden: "hidden",
  symbolic_link: "link, not followed",
  nested_folder: "folder inside a folder",
  unsupported_type: "not video, audio, or PDF",
  unreadable: "could not be read",
  no_lessons: "no video, audio, or PDF inside",
  limit_reached: "drop too large",
  no_target_lesson: "no lesson chosen",
};

/** The lesson under a drop point, found through `data-lesson-id` on lesson rows. */
function lessonAt(x: number, y: number): string | null {
  const element = typeof document.elementFromPoint === "function" ? document.elementFromPoint(x, y) : null;
  return element?.closest<HTMLElement>("[data-lesson-id]")?.dataset.lessonId ?? null;
}

function plural(count: number, noun: string) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function DropImport({ draft, writer, disabled }: { draft: DraftDetail; writer: DraftWriter; disabled: boolean }) {
  const [dragging, setDragging] = useState(false);
  const [pending, setPending] = useState<DropSummary | null>(null);
  const [lessonId, setLessonId] = useState<string>("");
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<DropOutcome | null>(null);
  const pendingToken = useRef<string | null>(null);
  const lessons = useMemo(
    () => draft.modules.flatMap((module) => module.lessons.map((lesson) => ({ id: lesson.id, label: `${module.title} › ${lesson.title}` }))),
    [draft.modules],
  );

  useEffect(() => {
    let active = true;
    const unlisten = [
      publisherApi.onDrag((value) => active && setDragging(value)),
      publisherApi.onDrop((summary) => {
        if (!active) return;
        if (pendingToken.current) void publisherApi.discardDrop(pendingToken.current).catch(() => undefined);
        pendingToken.current = summary.token;
        setPending(summary);
        setLessonId(summary.looseFiles.length ? (lessonAt(summary.x, summary.y) ?? "") : "");
        setError(null);
        setResult(null);
      }),
    ];
    return () => {
      active = false;
      for (const stop of unlisten) void stop.then((off) => off()).catch(() => undefined);
    };
  }, []);

  const dismiss = () => {
    if (pending) void publisherApi.discardDrop(pending.token).catch(() => undefined);
    pendingToken.current = null;
    setPending(null);
    setError(null);
  };

  const apply = async () => {
    if (!pending) return;
    setApplying(true);
    setError(null);
    let outcome: DropOutcome | null = null;
    try {
      await writer.submit(async (revision) => {
        outcome = await publisherApi.applyDrop(draft.draftId, revision, pending.token, lessonId || null);
        return outcome.draft;
      });
      pendingToken.current = null;
      setPending(null);
      setResult(outcome);
    } catch (failure) {
      const command = asCommandError(failure);
      if (command?.code !== "revision_conflict") setError(command?.message ?? "The dropped files could not be added.");
    } finally {
      setApplying(false);
    }
  };

  const lessonCount = pending?.modules.reduce((total, module) => total + module.lessons.length, 0) ?? 0;
  const looseCount = pending?.looseFiles.length ?? 0;
  const addsSomething = pending !== null && (pending.modules.length > 0 || (looseCount > 0 && lessonId !== ""));

  return (
    <>
      {dragging && !pending ? (
        <div role="status" className="flex items-center gap-3 rounded-3xl border-2 border-dashed border-amber-300/60 bg-amber-300/[0.06] p-5 text-sm text-amber-100">
          <FolderInput size={18} /> Drop folders to add them as modules, or drop files onto a lesson to attach them.
        </div>
      ) : null}

      {result ? (
        <p role="status" className="flex items-start justify-between gap-3 rounded-2xl border border-emerald-300/20 bg-emerald-300/[0.06] p-4 text-sm text-emerald-100">
          <span>
            Added {plural(result.modulesAdded, "module")} with {plural(result.lessonsAdded, "lesson")}
            {result.filesAttached ? ` and attached ${plural(result.filesAttached, "file")}` : ""}.
            {result.skipped.length ? ` ${plural(result.skipped.length, "item")} left out.` : ""}
          </span>
          <button type="button" aria-label="Dismiss import summary" onClick={() => setResult(null)} className="text-emerald-200/70 hover:text-emerald-100"><X size={15} /></button>
        </p>
      ) : null}

      {pending ? (
        <section role="dialog" aria-label="Add dropped files" className="space-y-4 rounded-3xl border border-amber-300/30 bg-amber-300/[0.05] p-6 text-sm">
          <h2 className="flex items-center gap-2 font-semibold"><FolderInput size={17} className="text-amber-300" /> Add dropped files</h2>

          {pending.modules.length ? (
            <div>
              <p>{plural(pending.modules.length, "folder")} become {plural(pending.modules.length, "new module")} with {plural(lessonCount, "lesson")}:</p>
              <ul aria-label="Modules to add" className="mt-2 space-y-2">
                {pending.modules.map((module, index) => (
                  <li key={`${module.title}-${index}`} className="rounded-xl border border-white/10 bg-black/10 px-3 py-2">
                    <details>
                      <summary className="cursor-pointer"><span className="font-medium">{module.title}</span> <span className="text-xs text-slate-400">· {plural(module.lessons.length, "lesson")}</span></summary>
                      <ol className="mt-2 list-decimal space-y-0.5 pl-6 text-xs text-slate-300">
                        {module.lessons.map((lesson, lessonIndex) => <li key={`${lesson.fileName}-${lessonIndex}`}>{lesson.title} <span className="text-slate-500">({lesson.fileName})</span></li>)}
                      </ol>
                    </details>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}

          {looseCount ? (
            <div>
              <label htmlFor="drop-target-lesson" className="block">
                Attach {plural(looseCount, "file")} ({pending.looseFiles.map((file) => file.fileName).join(", ")}) to
              </label>
              <select id="drop-target-lesson" value={lessonId} onChange={(event) => setLessonId(event.target.value)} className="mt-1 w-full max-w-md rounded-xl border border-white/10 bg-black/30 px-3 py-2 text-sm">
                <option value="">{lessons.length ? "Choose a lesson…" : "Add a lesson first"}</option>
                {lessons.map((lesson) => <option key={lesson.id} value={lesson.id}>{lesson.label}</option>)}
              </select>
            </div>
          ) : null}

          {pending.skipped.length ? (
            <details>
              <summary className="cursor-pointer text-xs text-slate-400">{plural(pending.skipped.length, "item")} left out</summary>
              <ul aria-label="Left out" className="mt-2 space-y-0.5 text-xs text-slate-400">
                {pending.skipped.map((entry, index) => <li key={`${entry.name}-${index}`}><span className="text-slate-300">{entry.name}</span> — {reasons[entry.reason]}</li>)}
              </ul>
            </details>
          ) : null}

          {!pending.modules.length && !looseCount ? <p className="text-slate-300">Nothing in this drop can be added.</p> : null}
          {error ? <p role="alert" className="text-xs text-red-200">{error}</p> : null}

          <div className="flex gap-2">
            <button type="button" onClick={() => void apply()} disabled={disabled || applying || !addsSomething} className="rounded-xl bg-amber-300 px-4 py-2 text-sm font-semibold text-slate-950 disabled:opacity-50">
              {applying ? "Adding…" : "Add to course"}
            </button>
            <button type="button" onClick={dismiss} disabled={applying} className="rounded-xl border border-white/10 px-4 py-2 text-sm">Dismiss</button>
          </div>
        </section>
      ) : null}
    </>
  );
}
