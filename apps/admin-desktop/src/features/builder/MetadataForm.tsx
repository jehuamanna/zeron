import { AlertTriangle, CheckCircle2, Loader2, RefreshCw } from "lucide-react";
import type { ReactNode } from "react";

import type { DraftDetail } from "../../services/publisher";
import { useMetadataAutosave, type MetadataFields, type SaveState } from "./useMetadataAutosave";

interface MetadataFormProps {
  draft: DraftDetail;
  onSaved: (saved: DraftDetail) => void;
  onReload: () => void;
  autosaveDelayMs?: number;
}

const inputClass =
  "mt-1.5 w-full rounded-xl border border-white/10 bg-black/30 px-3 py-2 text-sm text-slate-100 outline-none focus:border-amber-300/60 aria-[invalid=true]:border-red-400/70";

export function MetadataForm({ draft, onSaved, onReload, autosaveDelayMs = 700 }: MetadataFormProps) {
  const { fields, state, update, retry } = useMetadataAutosave(draft, onSaved, autosaveDelayMs);
  const invalidField = state.kind === "error" ? state.field : undefined;
  const control = (name: keyof MetadataFields, field: string) => ({
    id: `metadata-${name}`,
    value: fields[name],
    disabled: state.kind === "conflict",
    "aria-invalid": invalidField === field,
    className: inputClass,
    onChange: (event: { target: { value: string } }) => update(name, event.target.value),
  });

  return (
    <form onSubmit={(event) => event.preventDefault()} aria-label="Course details">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold">Course details</h2>
          <p className="mt-1 text-sm text-slate-400">Changes save automatically.</p>
        </div>
        <SaveStatus state={state} onRetry={retry} onReload={onReload} />
      </div>
      <div className="mt-6 grid gap-4 md:grid-cols-2">
        <Field label="Title" htmlFor="metadata-title" wide>
          <input {...control("title", "metadata.title")} />
        </Field>
        <Field label="Description" htmlFor="metadata-description" wide>
          <textarea rows={3} {...control("description", "metadata.description")} />
        </Field>
        <Field label="Release version" htmlFor="metadata-courseVersion">
          <input {...control("courseVersion", "metadata.version")} />
        </Field>
        <Field label="Language" htmlFor="metadata-language">
          <input {...control("language", "metadata.language")} />
        </Field>
        <Field label="Publisher name" htmlFor="metadata-publisherName">
          <input {...control("publisherName", "metadata.publisherName")} />
        </Field>
        <Field label="License" htmlFor="metadata-licenseName">
          <input {...control("licenseName", "metadata.licenseName")} />
        </Field>
        <Field label="Category" htmlFor="metadata-category">
          <input {...control("category", "metadata.category")} />
        </Field>
        <Field label="Tags (comma separated)" htmlFor="metadata-tags">
          <input {...control("tags", "metadata.tags")} />
        </Field>
        <Field label="Prerequisites (one per line)" htmlFor="metadata-prerequisites" wide>
          <textarea rows={2} {...control("prerequisites", "metadata.prerequisites")} />
        </Field>
      </div>
    </form>
  );
}

function Field({ label, htmlFor, wide, children }: { label: string; htmlFor: string; wide?: boolean; children: ReactNode }) {
  return (
    <div className={wide ? "md:col-span-2" : undefined}>
      <label htmlFor={htmlFor} className="text-xs font-medium uppercase tracking-wider text-slate-400">
        {label}
      </label>
      {children}
    </div>
  );
}

function SaveStatus({ state, onRetry, onReload }: { state: SaveState; onRetry: () => void; onReload: () => void }) {
  const base = "flex shrink-0 items-center gap-2 rounded-full px-3 py-1 text-xs";
  switch (state.kind) {
    case "saved":
      return (
        <span role="status" className={`${base} bg-emerald-300/10 text-emerald-300`}>
          <CheckCircle2 size={14} /> Saved · r{state.revision}
        </span>
      );
    case "dirty":
      return <span role="status" className={`${base} bg-white/10 text-slate-300`}>Unsaved changes</span>;
    case "saving":
      return (
        <span role="status" className={`${base} bg-white/10 text-slate-300`}>
          <Loader2 size={14} className="animate-spin" /> Saving…
        </span>
      );
    case "conflict":
      return (
        <div role="alert" className="max-w-xs rounded-2xl border border-amber-300/30 bg-amber-300/10 p-3 text-xs text-amber-100">
          <p className="flex items-center gap-2 font-semibold"><AlertTriangle size={14} /> Changed elsewhere</p>
          <p className="mt-1">
            A newer revision{state.currentRevision === undefined ? "" : ` (r${state.currentRevision})`} was saved. Your latest edits were not saved.
          </p>
          <button type="button" onClick={onReload} className="mt-2 flex items-center gap-1.5 rounded-lg bg-amber-300 px-2.5 py-1 font-semibold text-slate-950">
            <RefreshCw size={13} /> Discard my edits and load latest
          </button>
        </div>
      );
    case "error":
      return (
        <div role="alert" className="max-w-xs rounded-2xl border border-red-400/30 bg-red-400/10 p-3 text-xs text-red-100">
          <p className="font-semibold">Not saved</p>
          <p className="mt-1">{state.message}</p>
          <button type="button" onClick={onRetry} className="mt-2 rounded-lg bg-white/10 px-2.5 py-1 font-semibold">
            Retry
          </button>
        </div>
      );
  }
}
