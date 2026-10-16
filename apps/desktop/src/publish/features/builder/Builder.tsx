import { AlertTriangle, RefreshCw } from "lucide-react";
import { useRef, useState } from "react";

import type { DraftDetail } from "../../services/publisher";
import { createDraftWriter } from "./draftWriter";
import { DropImport } from "./DropImport";
import { MetadataForm } from "./MetadataForm";
import { StructureEditor } from "./StructureEditor";

interface BuilderProps {
  draft: DraftDetail;
  onSaved: (saved: DraftDetail) => void;
  onReload: () => void;
  autosaveDelayMs?: number;
}

/** One editing session for a loaded draft revision; remount it to start over after a reload. */
export function Builder({ draft, onSaved, onReload, autosaveDelayMs }: BuilderProps) {
  const [conflict, setConflict] = useState<{ currentRevision: number | undefined } | null>(null);
  const savedCallback = useRef(onSaved);
  savedCallback.current = onSaved;
  const [writer] = useState(() =>
    createDraftWriter(draft.revision, {
      onSaved: (saved) => savedCallback.current(saved),
      onConflict: (currentRevision) => setConflict({ currentRevision }),
    }),
  );
  const locked = conflict !== null;

  return (
    <div className="space-y-6">
      <p className="text-xs text-slate-500">Draft revision r{draft.revision}</p>
      {conflict ? (
        <div role="alert" className="flex items-center justify-between gap-4 rounded-2xl border border-amber-300/30 bg-amber-300/10 p-4 text-sm text-amber-100">
          <p className="flex items-center gap-2">
            <AlertTriangle size={16} className="shrink-0" />
            <span>
              This draft was changed elsewhere{conflict.currentRevision === undefined ? "" : ` (now r${conflict.currentRevision})`}. Your latest change was not saved and editing is paused.
            </span>
          </p>
          <button type="button" onClick={onReload} className="flex shrink-0 items-center gap-1.5 rounded-lg bg-accent hover:bg-accent-hover px-3 py-1.5 font-semibold text-slate-950">
            <RefreshCw size={14} /> Discard my edits and load latest
          </button>
        </div>
      ) : null}
      <div className="rounded-3xl border border-white/10 bg-white/[0.035] p-7">
        <MetadataForm draft={draft} writer={writer} disabled={locked} {...(autosaveDelayMs === undefined ? {} : { autosaveDelayMs })} />
      </div>
      <DropImport draft={draft} writer={writer} disabled={locked} />
      <div className="rounded-3xl border border-white/10 bg-white/[0.035] p-7">
        <StructureEditor draft={draft} writer={writer} disabled={locked} />
      </div>
    </div>
  );
}
