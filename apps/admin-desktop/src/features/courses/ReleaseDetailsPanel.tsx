import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Check, Copy, Download } from "lucide-react";
import { useRef, useState } from "react";

import { copyText } from "../../lib/clipboard";
import { formatBytes } from "../../lib/format";
import { asCommandError, publisherApi, type Release, type SeedingStatus } from "../../services/publisher";
import { ConfirmAction } from "./ConfirmAction";
import { seedingLabel } from "./labels";

type CopyState = "idle" | "copied" | "manual";

export function ReleaseDetailsPanel({ id, release, seeding }: { id: string; release: Release; seeding: SeedingStatus | "unavailable" | undefined }) {
  const details = useQuery({ queryKey: ["publisher-release-details", release.releaseId], queryFn: () => publisherApi.getReleaseDetails(release.releaseId) });
  const magnetField = useRef<HTMLInputElement>(null);
  const [copy, setCopy] = useState<CopyState>("idle");
  const exportTorrent = useMutation({ mutationFn: () => publisherApi.exportReleaseTorrent(release.releaseId) });
  const queryClient = useQueryClient();
  const [archiving, setArchiving] = useState(false);
  const archive = useMutation({
    mutationFn: () => publisherApi.archiveRelease(release.releaseId),
    onSuccess: () => {
      setArchiving(false);
      void queryClient.invalidateQueries({ queryKey: ["publisher-releases"] });
      void queryClient.invalidateQueries({ queryKey: ["distribution"] });
      void queryClient.invalidateQueries({ queryKey: ["publisher-release-details", release.releaseId] });
    },
  });
  const failure = seeding && seeding !== "unavailable" ? seeding.failure : null;

  const copyMagnet = async () => {
    setCopy((await copyText(release.magnet, magnetField.current)) ? "copied" : "manual");
  };

  return (
    <div id={id} className="space-y-4 border-t border-white/10 px-4 py-4">
      <div>
        <label htmlFor={`${id}-magnet`} className="text-xs font-semibold text-slate-400">Magnet link</label>
        <div className="mt-1 flex gap-2">
          <input id={`${id}-magnet`} ref={magnetField} readOnly value={release.magnet} onFocus={(event) => event.currentTarget.select()} className="min-w-0 flex-1 rounded-xl border border-white/10 bg-black/30 px-3 py-2 font-mono text-xs text-slate-200" />
          <button type="button" onClick={() => void copyMagnet()} className="flex items-center gap-1.5 rounded-xl border border-white/10 px-3 py-2 text-xs hover:border-amber-300/40">
            {copy === "copied" ? <Check size={14} className="text-emerald-300" /> : <Copy size={14} />} {copy === "copied" ? "Copied" : "Copy"}
          </button>
        </div>
        {copy === "manual" ? <p role="status" className="mt-1 text-xs text-amber-200">Copying is blocked here. The link is selected; press Ctrl+C to copy it.</p> : null}
      </div>

      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-1.5 text-xs">
        <dt className="text-slate-500">Info hash</dt>
        <dd><code className="break-all text-slate-300">{release.infoHash}</code></dd>
        <dt className="text-slate-500">Status</dt>
        <dd className="capitalize text-slate-300">{release.status}</dd>
        <dt className="text-slate-500">Seeding</dt>
        <dd className="text-slate-300">{seedingLabel(seeding)}{failure ? `: ${failure}` : ""}</dd>
        <dt className="text-slate-500">Size</dt>
        <dd className="text-slate-300">{formatBytes(release.totalBytes)} in {release.fileCount} files · {release.pieceCount} pieces of {formatBytes(release.pieceLength)}</dd>
        <dt className="text-slate-500">Published</dt>
        <dd className="text-slate-300">{new Date(release.publishedAtUnixSeconds * 1000).toLocaleString()}</dd>
      </dl>

      <div className="flex flex-wrap items-center gap-3">
        <button type="button" onClick={() => exportTorrent.mutate()} disabled={exportTorrent.isPending} className="flex items-center gap-2 rounded-xl bg-amber-300 px-3 py-2 text-xs font-semibold text-slate-950 disabled:opacity-50">
          <Download size={14} /> {exportTorrent.isPending ? "Exporting…" : "Export .torrent…"}
        </button>
        {exportTorrent.isSuccess && exportTorrent.data ? <p role="status" className="text-xs text-emerald-300">Torrent file saved.</p> : null}
        {exportTorrent.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(exportTorrent.error)?.message ?? "The torrent file could not be exported."}</p> : null}
      </div>

      {release.status === "archived" ? (
        <p className="text-xs text-slate-400">Archived. This release is no longer seeded; its files are kept on this computer.</p>
      ) : archiving ? (
        <ConfirmAction
          title={`Archive version ${release.courseVersion}?`}
          confirmLabel="Archive release"
          pendingLabel="Archiving…"
          pending={archive.isPending}
          error={archive.isError ? (asCommandError(archive.error)?.message ?? "The release could not be archived.") : null}
          onConfirm={() => archive.mutate()}
          onCancel={() => { archive.reset(); setArchiving(false); }}
        >
          Seeding stops now and will not resume after a restart. Learners who already have this version keep it. The release files stay on this computer, and the version number can never be reused.
        </ConfirmAction>
      ) : (
        <button type="button" onClick={() => setArchiving(true)} className="flex items-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-xs hover:border-red-400/40 hover:text-red-200">
          <Archive size={14} /> Archive release…
        </button>
      )}

      <div>
        <h4 className="text-xs font-semibold text-slate-400">Files</h4>
        {details.isPending ? <p className="mt-1 text-xs text-slate-500">Reading the release torrent…</p> : details.isError ? (
          <p role="alert" className="mt-1 text-xs text-red-300">{asCommandError(details.error)?.message ?? "The release files could not be read."}</p>
        ) : (
          <ul aria-label="Release files" className="mt-1 max-h-60 space-y-0.5 overflow-y-auto font-mono text-xs">
            {details.data.files.map((file) => (
              <li key={file.path} className="flex justify-between gap-4 text-slate-300">
                <span className="truncate" title={file.path}>{file.path}</span>
                <span className="shrink-0 text-slate-500">{formatBytes(file.byteLength)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
