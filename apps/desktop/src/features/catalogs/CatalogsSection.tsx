import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpen, ChevronDown, ChevronRight, Download, FileUp, Link, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type Catalog, type CatalogEntry } from "../../services/library";
import { useLibraryUi } from "../../stores/library";

const statusLabels: Record<CatalogEntry["status"], string> = {
  available: "",
  in_library: "In your library",
  update: "Update available",
  other_version: "You have another version",
  other_publisher: "You have this course from another publisher",
};

/**
 * Publisher catalogs: signed lists of courses to import. Imports go through
 * the usual magnet import, which verifies each release's own signature.
 */
export function CatalogsSection({ torrentsAvailable }: { torrentsAvailable: boolean }) {
  const queryClient = useQueryClient();
  const catalogs = useQuery({ queryKey: ["catalogs"], queryFn: libraryApi.listCatalogs });
  const [url, setUrl] = useState("");
  const [adding, setAdding] = useState(false);
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ["catalogs"] });
  const fromFile = useMutation({ mutationFn: libraryApi.chooseCatalogFile, onSuccess: (added) => { if (added) refresh(); } });
  const fromUrl = useMutation({
    mutationFn: () => libraryApi.importCatalogUrl(url),
    onSuccess: () => {
      setUrl("");
      setAdding(false);
      refresh();
    },
  });
  const failure = fromFile.error ?? fromUrl.error;
  const list = catalogs.data ?? [];

  return (
    <section aria-labelledby="catalogs-heading" className="mb-8 rounded-3xl border border-white/10 bg-white/[0.03] p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="catalogs-heading" className="flex items-center gap-2 font-semibold"><BookOpen size={17} className="text-cyan-300" /> Catalogs</h2>
        <div className="flex flex-wrap gap-2">
          <Button variant="ghost" onClick={() => fromFile.mutate()} disabled={fromFile.isPending}><FileUp size={15} /> Add catalog file…</Button>
          <Button variant="ghost" onClick={() => setAdding((value) => !value)}><Link size={15} /> Follow address…</Button>
        </div>
      </div>
      {!list.length ? <p className="mt-2 text-sm text-slate-500">A publisher's catalog lists their courses. Each one is checked against the publisher's signature.</p> : null}
      {adding ? (
        <form
          className="mt-3 flex flex-wrap gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            fromUrl.mutate();
          }}
        >
          <input aria-label="Catalog address" placeholder="https://…/zeron-catalog.json" value={url} onChange={(event) => setUrl(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1.5 text-sm" />
          <Button type="submit" disabled={!url.trim() || fromUrl.isPending}>{fromUrl.isPending ? "Downloading…" : "Follow"}</Button>
        </form>
      ) : null}
      {failure ? <p role="alert" className="mt-2 text-sm text-red-300">{asCommandError(failure)?.message ?? "The catalog could not be added."}</p> : null}
      {list.length ? (
        <ul aria-label="Followed catalogs" className="mt-4 space-y-3">
          {list.map((catalog) => <CatalogCard key={catalog.publisherId} catalog={catalog} torrentsAvailable={torrentsAvailable} onChanged={refresh} />)}
        </ul>
      ) : null}
    </section>
  );
}

function CatalogCard({ catalog, torrentsAvailable, onChanged }: { catalog: Catalog; torrentsAvailable: boolean; onChanged: () => void }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const { trackTorrentImport } = useLibraryUi();
  const refresh = useMutation({ mutationFn: () => libraryApi.refreshCatalog(catalog.publisherId), onSuccess: onChanged });
  const remove = useMutation({ mutationFn: () => libraryApi.removeCatalog(catalog.publisherId), onSuccess: onChanged });
  const importEntry = useMutation({
    mutationFn: (entry: CatalogEntry) => libraryApi.importMagnet(entry.magnet),
    onSuccess: (started, entry) => {
      queryClient.setQueryData(["torrent-import", started.jobId], started);
      trackTorrentImport(started.jobId, { kind: "magnet", uri: entry.magnet });
    },
  });
  const updates = catalog.entries.filter((entry) => entry.status === "update").length;
  const panel = `catalog-${catalog.publisherId}`;
  return (
    <li className="rounded-2xl border border-white/10 bg-black/10 text-sm">
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <button type="button" aria-expanded={open} aria-controls={panel} onClick={() => setOpen((value) => !value)} className="flex min-w-0 flex-1 items-center gap-2 text-left">
          {open ? <ChevronDown size={15} className="text-slate-400" /> : <ChevronRight size={15} className="text-slate-400" />}
          <span className="font-semibold">{catalog.title}</span>
          <code className="rounded bg-white/5 px-1.5 py-0.5 text-xs tracking-wider text-slate-300">{catalog.fingerprint}</code>
          <span className="text-xs text-slate-500">{catalog.entries.length} {catalog.entries.length === 1 ? "course" : "courses"}{updates ? ` · ${updates} ${updates === 1 ? "update" : "updates"}` : ""}</span>
        </button>
        {catalog.sourceUrl ? (
          <button type="button" aria-label={`Refresh ${catalog.title}`} onClick={() => refresh.mutate()} disabled={refresh.isPending} className="rounded-lg p-1.5 text-slate-400 hover:bg-white/10 disabled:opacity-40"><RefreshCw size={15} /></button>
        ) : null}
        <button type="button" aria-label={`Forget ${catalog.title}`} onClick={() => remove.mutate()} disabled={remove.isPending} className="rounded-lg p-1.5 text-slate-400 hover:bg-white/10 hover:text-red-200"><Trash2 size={15} /></button>
      </div>
      {refresh.isError ? <p role="alert" className="px-4 pb-2 text-xs text-red-300">{asCommandError(refresh.error)?.message ?? "The catalog could not be refreshed."}</p> : null}
      {open ? (
        <ul id={panel} aria-label={`${catalog.title} courses`} className="space-y-2 border-t border-white/10 px-4 py-3">
          {catalog.entries.map((entry) => (
            <li key={`${entry.courseId}-${entry.courseVersion}`} className="flex flex-wrap items-start gap-3">
              <div className="min-w-0 flex-1">
                <p className="font-medium">{entry.title} <span className="text-xs text-slate-500">version {entry.courseVersion}</span></p>
                {entry.description ? <p className="mt-0.5 line-clamp-2 text-xs text-slate-400">{entry.description}</p> : null}
                {statusLabels[entry.status] ? <p className={`mt-0.5 text-xs ${entry.status === "update" ? "text-cyan-300" : entry.status === "other_publisher" ? "text-amber-200" : "text-slate-500"}`}>{statusLabels[entry.status]}</p> : null}
              </div>
              {entry.status === "available" || entry.status === "update" ? (
                <Button onClick={() => importEntry.mutate(entry)} disabled={!torrentsAvailable || importEntry.isPending} className="shrink-0">
                  <Download size={15} /> {entry.status === "update" ? "Update" : "Import"}
                </Button>
              ) : null}
            </li>
          ))}
          {importEntry.isError ? <li role="alert" className="text-xs text-red-300">{asCommandError(importEntry.error)?.message ?? "The import could not start."}</li> : null}
        </ul>
      ) : null}
    </li>
  );
}
