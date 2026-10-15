import { useQuery } from "@tanstack/react-query";
import { AlertCircle, AlertTriangle, CheckCircle2, Info, ScanSearch } from "lucide-react";

import { publisherApi } from "../../services/publisher";

interface ReadinessPanelProps {
  draftId: string;
  revision: number;
}

export function ReadinessPanel({ draftId, revision }: ReadinessPanelProps) {
  const validation = useQuery({
    queryKey: ["publisher-draft-validation", draftId],
    queryFn: () => publisherApi.validateDraft(draftId),
  });
  const media = useQuery({
    queryKey: ["publisher-draft-media", draftId, revision],
    queryFn: () => publisherApi.inspectMedia(draftId),
    enabled: false,
  });

  return (
    <aside className="h-fit space-y-6 rounded-3xl border border-white/10 bg-black/20 p-6">
      <section aria-labelledby="readiness-heading">
        <h2 id="readiness-heading" className="font-semibold">Publishing readiness</h2>
        {validation.isPending ? <p className="mt-5 text-sm text-slate-400">Checking draft…</p> : validation.isError ? (
          <p role="alert" className="mt-5 text-sm text-red-300">Validation is unavailable.</p>
        ) : validation.data.length ? (
          <ul aria-label="Blocking issues" className="mt-5 space-y-3 text-sm">
            {validation.data.map((issue) => (
              <li key={`${issue.field}:${issue.code}`} className="flex gap-3 text-slate-300">
                <AlertCircle className="mt-0.5 shrink-0 text-amber-300" size={16} />
                <span><span className="block font-mono text-xs text-slate-500">{issue.field}</span>{issue.message}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="mt-5 flex gap-3 text-sm text-slate-400"><CheckCircle2 className="shrink-0 text-emerald-300" size={18} /><span>No blocking draft issues. Packaging checks run when publishing.</span></div>
        )}
      </section>

      <section aria-labelledby="media-heading" className="border-t border-white/10 pt-5">
        <div className="flex items-center justify-between gap-3">
          <h2 id="media-heading" className="font-semibold">Media checks</h2>
          <button
            type="button"
            onClick={() => void media.refetch()}
            disabled={media.isFetching}
            className="flex items-center gap-1.5 rounded-lg bg-white/10 px-2.5 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-50"
          >
            <ScanSearch size={14} /> {media.isFetching ? "Checking…" : media.data ? "Check again" : "Check media"}
          </button>
        </div>
        {media.isError ? (
          <p role="alert" className="mt-4 text-sm text-red-300">Media could not be checked.</p>
        ) : !media.data ? (
          <p className="mt-4 text-sm text-slate-500">Reads only file headers to find playback problems. Files are never changed.</p>
        ) : media.data.length ? (
          <ul aria-label="Media findings" className="mt-4 space-y-3 text-sm">
            {media.data.map((finding) => (
              <li key={`${finding.resourceId}:${finding.code}`} className="flex gap-3 text-slate-300">
                {finding.severity === "warning" ? (
                  <AlertTriangle aria-label="Warning" className="mt-0.5 shrink-0 text-amber-300" size={16} />
                ) : (
                  <Info aria-label="Advice" className="mt-0.5 shrink-0 text-sky-300" size={16} />
                )}
                <span><span className="block truncate text-xs text-slate-500">{finding.fileName}</span>{finding.message}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="mt-4 flex gap-3 text-sm text-slate-400"><CheckCircle2 className="shrink-0 text-emerald-300" size={18} /><span>No media issues found.</span></div>
        )}
      </section>
    </aside>
  );
}
