import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi } from "../../services/library";
import { formatSize } from "../downloads/downloads";

const GIB = 1024 ** 3;

/** Empty means no cap; otherwise at least 0.25 GB. */
export function parseCap(text: string): number | null | undefined {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  const value = Number(trimmed);
  return Number.isFinite(value) && value >= 0.25 && value <= 100_000 ? Math.round(value * GIB) : undefined;
}

export function StorageSettings() {
  const queryClient = useQueryClient();
  const storage = useQuery({ queryKey: ["storage"], queryFn: libraryApi.storage });
  const save = useMutation({
    mutationFn: (limit: number | null) => libraryApi.setCacheLimit(limit),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["storage"] }),
  });
  if (storage.isPending) return null;
  if (storage.isError) return <p className="mt-6 text-xs text-slate-500">Storage use is unavailable while the torrent engine is not running.</p>;
  return <StorageForm key={String(storage.data.cacheLimitBytes)} streamedBytes={storage.data.streamedBytes} limitBytes={storage.data.cacheLimitBytes} saving={save.isPending} error={save.isError ? (asCommandError(save.error)?.message ?? "The limit could not be saved.") : null} freed={save.isSuccess ? save.data.freedBytes : null} onSave={(limit) => save.mutate(limit)} />;
}

function StorageForm({ streamedBytes, limitBytes, saving, error, freed, onSave }: {
  streamedBytes: number;
  limitBytes: number | null;
  saving: boolean;
  error: string | null;
  freed: number | null;
  onSave: (limit: number | null) => void;
}) {
  const [value, setValue] = useState(limitBytes === null ? "" : String(Math.round((limitBytes / GIB) * 100) / 100));
  const [invalid, setInvalid] = useState(false);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const parsed = parseCap(value);
    setInvalid(parsed === undefined);
    if (parsed !== undefined) onSave(parsed);
  };
  return (
    <form onSubmit={submit} aria-labelledby="storage-heading" className="mt-6 border-t border-white/10 pt-5">
      <h2 id="storage-heading" className="font-medium">Storage</h2>
      <p className="mt-1 text-xs text-slate-400">Streamed lessons that are not kept offline use {formatSize(streamedBytes)}. Above the limit, the least recently played are removed; kept lessons never are.</p>
      <div className="mt-3 flex flex-wrap items-end gap-4">
        <label className="text-xs text-slate-300">
          Limit for streamed lessons (GB)
          <input inputMode="decimal" value={value} onChange={(event) => setValue(event.target.value)} placeholder="No limit" className="mt-1 block w-32 rounded-xl border border-white/10 bg-black/30 px-3 py-2 text-sm text-slate-100" />
        </label>
        <Button type="submit" variant="ghost" disabled={saving}>{saving ? "Saving…" : "Save limit"}</Button>
      </div>
      {invalid ? <p role="alert" className="mt-2 text-xs text-red-300">Enter at least 0.25 GB, or leave the field empty for no limit.</p> : null}
      {error ? <p role="alert" className="mt-2 text-xs text-red-300">{error}</p> : null}
      {freed !== null && freed > 0 ? <p role="status" className="mt-2 text-xs text-slate-400">Freed {formatSize(freed)}.</p> : null}
    </form>
  );
}
