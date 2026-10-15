import { useEffect, useRef, type ReactNode } from "react";

/** Inline confirmation for an action that cannot be undone from this screen. */
export function ConfirmAction({ title, children, confirmLabel, pendingLabel, pending, error, onConfirm, onCancel, confirmDisabled = false }: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  pendingLabel: string;
  pending: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
  /** Keeps the confirm button disabled, for example until a typed confirmation matches. */
  confirmDisabled?: boolean;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => cancel.current?.focus(), []);
  return (
    <div role="alertdialog" aria-label={title} className="rounded-2xl border border-red-400/30 bg-red-400/[0.06] p-4 text-sm">
      <p className="font-semibold text-red-100">{title}</p>
      <div className="mt-1 text-xs text-slate-300">{children}</div>
      {error ? <p role="alert" className="mt-2 text-xs text-red-200">{error}</p> : null}
      <div className="mt-3 flex gap-2">
        <button type="button" onClick={onConfirm} disabled={pending || confirmDisabled} className="rounded-xl bg-red-400 px-3 py-1.5 text-xs font-semibold text-slate-950 disabled:opacity-50">
          {pending ? pendingLabel : confirmLabel}
        </button>
        <button type="button" ref={cancel} onClick={onCancel} disabled={pending} className="rounded-xl border border-white/10 px-3 py-1.5 text-xs disabled:opacity-50">
          Cancel
        </button>
      </div>
    </div>
  );
}
