import { useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";

/** Smallest cap accepted by the backend, in KiB/s. */
const MIN_KIB = 16;

/** Empty means unlimited; otherwise a whole number of at least MIN_KIB. */
export function parseLimit(text: string): number | null | undefined {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return value >= MIN_KIB && value <= 4_000_000 ? value : undefined;
}

export function SpeedLimitsForm({ downloadKib, uploadKib, saving, onSave }: {
  downloadKib: number | null;
  uploadKib: number | null;
  saving: boolean;
  onSave: (downloadKib: number | null, uploadKib: number | null) => void;
}) {
  const [download, setDownload] = useState(downloadKib?.toString() ?? "");
  const [upload, setUpload] = useState(uploadKib?.toString() ?? "");
  const [invalid, setInvalid] = useState(false);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const [parsedDownload, parsedUpload] = [parseLimit(download), parseLimit(upload)];
    if (parsedDownload === undefined || parsedUpload === undefined) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    onSave(parsedDownload, parsedUpload);
  };
  const field = "mt-1 w-32 rounded-xl border border-white/10 bg-black/30 px-3 py-2 text-sm text-slate-100";
  return (
    <form onSubmit={submit} aria-labelledby="speed-heading" className="mt-6 border-t border-white/10 pt-5">
      <h2 id="speed-heading" className="font-medium">Speed limits</h2>
      <p className="mt-1 text-xs text-slate-400">In KiB/s. Leave a field empty for no limit. Changes apply immediately.</p>
      <div className="mt-3 flex flex-wrap items-end gap-4">
        <label className="text-xs text-slate-300">
          Download limit
          <input inputMode="numeric" value={download} onChange={(event) => setDownload(event.target.value)} placeholder="No limit" className={`block ${field}`} />
        </label>
        <label className="text-xs text-slate-300">
          Upload limit (shared courses)
          <input inputMode="numeric" value={upload} onChange={(event) => setUpload(event.target.value)} placeholder="No limit" className={`block ${field}`} />
        </label>
        <Button type="submit" variant="ghost" disabled={saving}>{saving ? "Saving…" : "Save limits"}</Button>
      </div>
      {invalid ? <p role="alert" className="mt-2 text-xs text-red-300">Enter a whole number of at least {MIN_KIB} KiB/s, or leave the field empty.</p> : null}
    </form>
  );
}
