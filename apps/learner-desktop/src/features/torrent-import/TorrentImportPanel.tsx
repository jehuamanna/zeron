import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, FileDown, Link2, LoaderCircle, RotateCcw, X } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";
import {
  asCommandError,
  finishedTorrentImportStages,
  libraryApi,
  type TorrentImport,
  type TorrentImportStage,
} from "../../services/library";
import { useLibraryUi, type TorrentImportSource } from "../../stores/library";

const steps: Array<{ stage: TorrentImportStage; label: string }> = [
  { stage: "resolving_metadata", label: "Find torrent metadata" },
  { stage: "fetching_manifest", label: "Download course.json" },
  { stage: "validating", label: "Check course structure" },
  { stage: "imported", label: "Add to library" },
];

const formatKilobytes = (bytes: number) =>
  new Intl.NumberFormat(undefined, { style: "unit", unit: "kilobyte", maximumFractionDigits: 1 }).format(bytes / 1000);

export function TorrentImportPanel({ engine }: { engine: "ready" | "unavailable" | "pending" }) {
  const available = engine === "ready";
  const queryClient = useQueryClient();
  const { torrentImport, trackTorrentImport, dismissTorrentImport } = useLibraryUi();
  const [magnetOpen, setMagnetOpen] = useState(false);
  const [magnet, setMagnet] = useState("");
  const jobId = torrentImport?.jobId ?? null;

  const job = useQuery({
    queryKey: ["torrent-import", jobId],
    queryFn: () => libraryApi.getTorrentImport(jobId!),
    enabled: jobId !== null,
    refetchInterval: (query) => {
      const stage = query.state.data?.stage;
      return stage && finishedTorrentImportStages.has(stage) ? false : 1000;
    },
  });
  const start = useMutation({
    mutationFn: (source: TorrentImportSource) =>
      source.kind === "file" ? libraryApi.chooseTorrentFile() : libraryApi.importMagnet(source.uri),
    onSuccess: (started, source) => {
      if (!started) return;
      queryClient.setQueryData(["torrent-import", started.jobId], started);
      trackTorrentImport(started.jobId, source);
      setMagnetOpen(false);
      setMagnet("");
    },
  });
  const cancel = useMutation({ mutationFn: (id: string) => libraryApi.cancelTorrentImport(id) });

  useEffect(() => {
    if (!jobId) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void libraryApi
      .onTorrentImport((event) => {
        if (event.jobId !== jobId) return;
        queryClient.setQueryData<TorrentImport | null>(["torrent-import", jobId], (current) =>
          current && current.sequence >= event.sequence ? current : event,
        );
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [jobId, queryClient]);

  const current = job.data ?? null;
  const imported = current?.stage === "imported";
  useEffect(() => {
    if (imported) void queryClient.invalidateQueries({ queryKey: ["courses"] });
  }, [imported, queryClient]);

  const running = current !== null && !finishedTorrentImportStages.has(current.stage);
  const busy = start.isPending || running;
  const startError = start.isError ? (asCommandError(start.error)?.message ?? "The torrent import could not start.") : null;

  const submitMagnet = (event: FormEvent) => {
    event.preventDefault();
    if (magnet.trim()) start.mutate({ kind: "magnet", uri: magnet.trim() });
  };

  return (
    <section aria-labelledby="torrent-import-heading" className="mb-8 rounded-3xl border border-white/10 bg-white/[0.03] p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 id="torrent-import-heading" className="font-semibold">Add a course from a torrent</h2>
          <p className="mt-1 text-sm text-slate-400">
            {engine === "unavailable"
              ? "Torrent import is unavailable because the torrent engine could not start. Local folders still work."
              : "Zeron downloads and checks only course.json first. Lesson media is not downloaded yet."}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="ghost" disabled={!available || busy} onClick={() => start.mutate({ kind: "file" })}>
            <FileDown size={17} /> Open .torrent file
          </Button>
          <Button variant="ghost" disabled={!available || busy} onClick={() => setMagnetOpen((open) => !open)} aria-expanded={magnetOpen}>
            <Link2 size={17} /> Paste magnet link
          </Button>
        </div>
      </div>

      {magnetOpen && available ? (
        <form onSubmit={submitMagnet} className="mt-4 flex flex-col gap-2 sm:flex-row">
          <label htmlFor="magnet-link" className="sr-only">Magnet link</label>
          <input
            id="magnet-link"
            value={magnet}
            onChange={(event) => setMagnet(event.target.value)}
            placeholder="magnet:?xt=urn:btih:…"
            spellCheck={false}
            autoComplete="off"
            className="min-w-0 flex-1 rounded-xl border border-white/10 bg-black/30 px-3 py-2.5 font-mono text-sm text-slate-100 placeholder:text-slate-600 focus:border-cyan-300/60 focus:outline-none"
          />
          <Button type="submit" disabled={busy || !magnet.trim()}>Import</Button>
        </form>
      ) : null}

      {startError ? <p role="alert" className="mt-4 text-sm text-red-300">{startError}</p> : null}

      {current ? (
        <div aria-live="polite" className="mt-5 rounded-2xl border border-white/10 bg-black/20 p-4">
          {current.stage === "failed" || current.stage === "cancelled" ? (
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div role={current.stage === "failed" ? "alert" : undefined}>
                <p className="font-medium">{current.stage === "failed" ? "Import failed" : "Import cancelled"}</p>
                <p className="mt-1 text-sm text-slate-400">
                  {current.failure?.message ?? "Nothing was added to your library."}
                </p>
              </div>
              <div className="flex gap-2">
                {torrentImport ? (
                  <Button variant="ghost" disabled={busy || !available} onClick={() => start.mutate(torrentImport.source)}>
                    <RotateCcw size={16} /> {torrentImport.source.kind === "file" ? "Choose file again" : "Retry"}
                  </Button>
                ) : null}
                <Button variant="ghost" onClick={dismissTorrentImport}>Dismiss</Button>
              </div>
            </div>
          ) : (
            <>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <p className="font-medium">
                  {imported ? `Added ${current.title ?? "course"} to your library` : "Importing from torrent"}
                </p>
                {running ? (
                  <Button variant="ghost" disabled={cancel.isPending} onClick={() => cancel.mutate(current.jobId)}>
                    <X size={16} /> Cancel
                  </Button>
                ) : (
                  <Button variant="ghost" onClick={dismissTorrentImport}>Dismiss</Button>
                )}
              </div>
              <ol className="mt-4 grid gap-2 sm:grid-cols-4">
                {steps.map((step, index) => {
                  const position = Math.max(0, steps.findIndex((candidate) => candidate.stage === current.stage));
                  const done = imported || index < position;
                  const active = !imported && index === position;
                  return (
                    <li
                      key={step.stage}
                      aria-current={active ? "step" : undefined}
                      className={`flex items-center gap-2 rounded-xl px-3 py-2 text-sm ${active ? "bg-cyan-300/10 text-cyan-200" : done ? "text-slate-200" : "text-slate-500"}`}
                    >
                      {done ? <Check size={15} /> : active ? <LoaderCircle size={15} className="animate-spin" /> : <span className="size-[15px]" />}
                      {step.label}
                    </li>
                  );
                })}
              </ol>
              {current.stage === "fetching_manifest" && current.manifestTotalBytes ? (
                <p className="mt-3 text-xs text-slate-400">
                  course.json: {formatKilobytes(current.manifestVerifiedBytes ?? 0)} of {formatKilobytes(current.manifestTotalBytes)} verified
                </p>
              ) : null}
            </>
          )}
        </div>
      ) : null}
    </section>
  );
}
