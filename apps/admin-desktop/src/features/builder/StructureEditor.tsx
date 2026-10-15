import { ArrowDown, ArrowUp, FilePlus2, FolderPlus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { asCommandError, publisherApi, type DraftDetail, type StructureEdit } from "../../services/publisher";
import type { DraftWriter } from "./draftWriter";

interface StructureEditorProps {
  draft: DraftDetail;
  writer: DraftWriter;
  disabled: boolean;
}

const iconButton =
  "grid size-8 place-items-center rounded-lg text-slate-400 hover:bg-white/10 hover:text-white disabled:pointer-events-none disabled:opacity-30";
const titleInput =
  "min-w-0 flex-1 rounded-lg border border-transparent bg-transparent px-2 py-1 outline-none hover:border-white/10 focus:border-amber-300/60 focus:bg-black/30";

export function StructureEditor({ draft, writer, disabled }: StructureEditorProps) {
  const pending = useRef(0);
  const [busy, setBusy] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const apply = async (edit: StructureEdit, announce: string) => {
    if (disabled) return;
    pending.current += 1;
    setBusy(true);
    setError(null);
    setConfirmingId(null);
    try {
      await writer.submit((revision) => publisherApi.editDraftStructure(draft.draftId, revision, edit));
      setAnnouncement(announce);
    } catch (failure) {
      const command = asCommandError(failure);
      if (command?.code !== "revision_conflict") setError(command?.message ?? "The change could not be saved.");
    } finally {
      pending.current -= 1;
      setBusy(false);
    }
  };

  const moduleCount = draft.modules.length;

  return (
    <section aria-labelledby="structure-heading" aria-busy={busy}>
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 id="structure-heading" className="text-lg font-semibold">Structure</h2>
          <p className="mt-1 text-sm text-slate-400">Reorder with the arrow buttons or Alt+↑/↓ while editing a title.</p>
        </div>
        <button
          type="button"
          disabled={disabled}
          onClick={() => void apply({ type: "addModule", title: "Untitled module" }, `Added module ${moduleCount + 1}.`)}
          className="flex items-center gap-2 rounded-xl bg-white/10 px-3 py-2 text-sm font-semibold hover:bg-white/15 disabled:opacity-40"
        >
          <FolderPlus size={16} /> Add module
        </button>
      </div>
      <p aria-live="polite" className="sr-only">{announcement}</p>
      {error ? <p role="alert" className="mt-4 rounded-xl border border-red-400/30 bg-red-400/10 p-3 text-sm text-red-200">{error}</p> : null}

      {moduleCount === 0 ? (
        <div className="mt-5 rounded-2xl border border-dashed border-white/15 py-12 text-center text-slate-500">
          <p>Add your first module, then add lessons to it.</p>
        </div>
      ) : (
        <ol className="mt-5 space-y-4">
          {draft.modules.map((module, moduleIndex) => {
            const moduleLabel = `module ${moduleIndex + 1}`;
            const moveModule = (offset: number, title = module.title) => {
              const position = moduleIndex + offset;
              if (position < 0 || position >= moduleCount) return;
              void apply(
                { type: "moveModule", moduleId: module.id, position },
                `Moved “${title}” to position ${position + 1} of ${moduleCount}.`,
              );
            };
            const lessonCount = module.lessons.length;
            return (
              <li key={module.id} className="rounded-2xl border border-white/10 bg-black/10 p-4">
                <div className="flex items-center gap-2">
                  <span className="w-6 shrink-0 text-center text-xs font-semibold text-amber-300">{moduleIndex + 1}</span>
                  <TitleInput
                    label={`Title of ${moduleLabel}`}
                    title={module.title}
                    disabled={disabled}
                    className={`${titleInput} font-semibold`}
                    onCommit={(title) => void apply({ type: "renameModule", moduleId: module.id, title }, `Renamed ${moduleLabel}.`)}
                    onMove={moveModule}
                  />
                  <button type="button" aria-label={`Move ${moduleLabel} up`} disabled={disabled || moduleIndex === 0} onClick={() => moveModule(-1)} className={iconButton}><ArrowUp size={16} /></button>
                  <button type="button" aria-label={`Move ${moduleLabel} down`} disabled={disabled || moduleIndex === moduleCount - 1} onClick={() => moveModule(1)} className={iconButton}><ArrowDown size={16} /></button>
                  <RemoveControl
                    label={moduleLabel}
                    detail={lessonCount ? `and its ${lessonCount} ${lessonCount === 1 ? "lesson" : "lessons"}` : null}
                    confirming={confirmingId === module.id}
                    disabled={disabled}
                    onRequest={() => (lessonCount ? setConfirmingId(module.id) : void apply({ type: "removeModule", moduleId: module.id }, `Removed ${moduleLabel}.`))}
                    onConfirm={() => void apply({ type: "removeModule", moduleId: module.id }, `Removed ${moduleLabel}.`)}
                    onCancel={() => setConfirmingId(null)}
                  />
                </div>

                <ol className="mt-3 space-y-1 pl-8">
                  {module.lessons.map((lesson, lessonIndex) => {
                    const lessonLabel = `lesson ${lessonIndex + 1} in ${moduleLabel}`;
                    const moveLesson = (offset: number, title = lesson.title) => {
                      const position = lessonIndex + offset;
                      if (position < 0 || position >= lessonCount) return;
                      void apply(
                        { type: "moveLesson", moduleId: module.id, lessonId: lesson.id, position },
                        `Moved “${title}” to position ${position + 1} of ${lessonCount}.`,
                      );
                    };
                    return (
                      <li key={lesson.id} className="flex items-center gap-2 rounded-xl px-1 py-1 hover:bg-white/[0.03]">
                        <span className="w-6 shrink-0 text-center text-xs text-slate-500">{lessonIndex + 1}</span>
                        <TitleInput
                          label={`Title of ${lessonLabel}`}
                          title={lesson.title}
                          disabled={disabled}
                          className={`${titleInput} text-sm`}
                          onCommit={(title) => void apply({ type: "renameLesson", lessonId: lesson.id, title }, `Renamed ${lessonLabel}.`)}
                          onMove={moveLesson}
                        />
                        <span className="shrink-0 text-xs text-slate-500">{lesson.resourceCount} {lesson.resourceCount === 1 ? "resource" : "resources"}</span>
                        <button type="button" aria-label={`Move ${lessonLabel} up`} disabled={disabled || lessonIndex === 0} onClick={() => moveLesson(-1)} className={iconButton}><ArrowUp size={15} /></button>
                        <button type="button" aria-label={`Move ${lessonLabel} down`} disabled={disabled || lessonIndex === lessonCount - 1} onClick={() => moveLesson(1)} className={iconButton}><ArrowDown size={15} /></button>
                        <RemoveControl
                          label={lessonLabel}
                          detail={lesson.resourceCount ? `and its ${lesson.resourceCount} resource ${lesson.resourceCount === 1 ? "reference" : "references"}` : null}
                          confirming={confirmingId === lesson.id}
                          disabled={disabled}
                          onRequest={() => (lesson.resourceCount ? setConfirmingId(lesson.id) : void apply({ type: "removeLesson", lessonId: lesson.id }, `Removed ${lessonLabel}.`))}
                          onConfirm={() => void apply({ type: "removeLesson", lessonId: lesson.id }, `Removed ${lessonLabel}.`)}
                          onCancel={() => setConfirmingId(null)}
                        />
                      </li>
                    );
                  })}
                </ol>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => void apply({ type: "addLesson", moduleId: module.id, title: "Untitled lesson" }, `Added lesson ${lessonCount + 1} to ${moduleLabel}.`)}
                  className="mt-2 ml-8 flex items-center gap-2 rounded-lg px-2 py-1 text-sm text-slate-400 hover:bg-white/10 hover:text-white disabled:opacity-40"
                  aria-label={`Add lesson to ${moduleLabel}`}
                >
                  <FilePlus2 size={15} /> Add lesson
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}

interface TitleInputProps {
  label: string;
  title: string;
  disabled: boolean;
  className: string;
  onCommit: (title: string) => void;
  onMove: (offset: number, title: string) => void;
}

function TitleInput({ label, title, disabled, className, onCommit, onMove }: TitleInputProps) {
  const [value, setValue] = useState(title);
  const [focused, setFocused] = useState(false);
  const committed = useRef(title);

  useEffect(() => {
    committed.current = title;
    if (!focused) setValue(title);
  }, [title, focused]);

  const commit = () => {
    if (value === committed.current) return;
    committed.current = value;
    onCommit(value);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    } else if (event.key === "Escape") {
      setValue(committed.current);
    } else if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      commit();
      onMove(event.key === "ArrowUp" ? -1 : 1, value);
    }
  };

  return (
    <input
      aria-label={label}
      value={value}
      disabled={disabled}
      maxLength={200}
      className={className}
      onChange={(event) => setValue(event.target.value)}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        setFocused(false);
        commit();
      }}
      onKeyDown={onKeyDown}
    />
  );
}

interface RemoveControlProps {
  label: string;
  detail: string | null;
  confirming: boolean;
  disabled: boolean;
  onRequest: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}

function RemoveControl({ label, detail, confirming, disabled, onRequest, onConfirm, onCancel }: RemoveControlProps) {
  const trigger = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);

  useEffect(() => {
    if (!confirming && restoreFocus.current) {
      restoreFocus.current = false;
      trigger.current?.focus();
    }
  }, [confirming]);

  const cancel = () => {
    restoreFocus.current = true;
    onCancel();
  };

  if (!confirming) {
    return (
      <button ref={trigger} type="button" aria-label={`Remove ${label}`} disabled={disabled} onClick={onRequest} className={`${iconButton} hover:text-red-300`}>
        <Trash2 size={15} />
      </button>
    );
  }
  return (
    <div
      role="group"
      aria-label={`Confirm removing ${label}`}
      className="flex shrink-0 items-center gap-1 text-xs"
      onKeyDown={(event) => {
        if (event.key === "Escape") cancel();
      }}
    >
      <span className="text-red-200">Remove {detail}?</span>
      <button type="button" onClick={onConfirm} className="rounded-lg bg-red-400/80 px-2 py-1 font-semibold text-slate-950">Remove</button>
      <button type="button" autoFocus onClick={cancel} className="rounded-lg bg-white/10 px-2 py-1">Cancel</button>
    </div>
  );
}
