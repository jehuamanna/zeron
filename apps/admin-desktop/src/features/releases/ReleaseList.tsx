import { useQuery } from "@tanstack/react-query";
import { Lock } from "lucide-react";

import { formatBytes } from "../../lib/format";
import { publisherApi, type Release } from "../../services/publisher";

const statusStyles: Record<Release["status"], string> = {
  published: "bg-emerald-300/10 text-emerald-300",
  superseded: "bg-white/10 text-slate-300",
  archived: "bg-white/5 text-slate-500",
};

export function ReleaseList() {
  const releases = useQuery({ queryKey: ["publisher-releases"], queryFn: publisherApi.listReleases });

  return (
    <section aria-labelledby="releases-heading" className="mt-12">
      <h2 id="releases-heading" className="flex items-center gap-2 text-lg font-semibold"><Lock size={17} className="text-slate-400" /> Releases</h2>
      <p className="mt-1 text-sm text-slate-500">Published versions are immutable and kept separately from drafts.</p>
      {releases.isPending ? <p className="mt-4 text-sm text-slate-400">Loading releases…</p> : releases.isError ? (
        <p role="alert" className="mt-4 text-sm text-red-300">Releases are unavailable.</p>
      ) : releases.data.length ? (
        <ul aria-label="Release history" className="mt-4 space-y-3">
          {releases.data.map((release) => (
            <li key={release.releaseId} className="flex flex-wrap items-center gap-x-6 gap-y-2 rounded-2xl border border-white/10 bg-white/[0.035] px-5 py-4 text-sm">
              <div className="min-w-0 flex-1">
                <p className="truncate font-semibold">{release.title}</p>
                <p className="text-xs text-slate-500">
                  Version {release.courseVersion} · {new Date(release.publishedAtUnixSeconds * 1000).toLocaleDateString()} · {release.fileCount} files · {formatBytes(release.totalBytes)}
                </p>
              </div>
              <code title={release.infoHash} className="text-xs text-slate-400">{release.infoHash.slice(0, 12)}…</code>
              <span className={`rounded-full px-2.5 py-0.5 text-xs capitalize ${statusStyles[release.status]}`}>{release.status}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-4 text-sm text-slate-500">No releases yet. Prepare and publish a draft to create one.</p>
      )}
    </section>
  );
}
