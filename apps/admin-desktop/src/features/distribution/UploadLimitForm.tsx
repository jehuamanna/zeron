import { useState, type FormEvent } from "react";

const MIN_KIB = 16;

/** Empty means unlimited; otherwise a whole number of at least MIN_KIB. */
export function parseLimit(text: string): number | null | undefined {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return value >= MIN_KIB && value <= 4_000_000 ? value : undefined;
}

export function UploadLimitForm({ uploadKib, saving, onSave }: { uploadKib: number | null; saving: boolean; onSave: (uploadKib: number | null) => void }) {
  const [value, setValue] = useState(uploadKib?.toString() ?? "");
  const [invalid, setInvalid] = useState(false);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const parsed = parseLimit(value);
    setInvalid(parsed === undefined);
    if (parsed !== undefined) onSave(parsed);
  };
  return (
    <form onSubmit={submit} className="mt-5 border-t border-white/10 pt-4">
      <label className="block text-sm font-medium">
        Upload limit
        <span className="mt-1 block text-xs font-normal text-slate-400">KiB/s shared by every release you seed. Leave empty for no limit. Applies immediately.</span>
        <input inputMode="numeric" value={value} onChange={(event) => setValue(event.target.value)} placeholder="No limit" className="mt-2 w-36 rounded-lg border border-white/10 bg-black/30 px-3 py-1.5 text-sm" />
      </label>
      <button type="submit" disabled={saving} className="mt-2 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
        {saving ? "Saving…" : "Save upload limit"}
      </button>
      {invalid ? <p role="alert" className="mt-2 text-xs text-red-200">Enter a whole number of at least {MIN_KIB} KiB/s, or leave it empty.</p> : null}
    </form>
  );
}
