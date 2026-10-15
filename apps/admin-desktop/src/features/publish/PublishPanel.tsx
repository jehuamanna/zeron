import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, CircleDashed, Loader2, PackageCheck, XCircle } from "lucide-react";
import { useEffect } from "react";

import { formatBytes } from "../../lib/format";
import { asCommandError, finishedPublishStages, publisherApi, type PublishJob, type PublishStage } from "../../services/publisher";
import { useBuilderUi } from "../../stores/builder";

interface PublishPanelProps {
  draftId: string;
  revision: number;
}

const steps: Array<{ stage: PublishStage; label: string }> = [
  { stage: "VALIDATING", label: "Validate draft" },
  { stage: "PACKAGING", label: "Copy files and write course.json" },
  { stage: "GENERATING_TORRENT", label: "Hash pieces" },
  { stage: "VERIFYING", label: "Verify torrent independently" },
  { stage: "READY_TO_PUBLISH", label: "Ready to activate" },
];

const stageOrder: PublishStage[] = steps.map((step) => step.stage);

export function PublishPanel({ draftId, revision }: PublishPanelProps) {
  const queryClient = useQueryClient();
  const jobId = useBuilderUi((state) => state.publishJobIds[draftId] ?? null);
  const rememberPublishJob = useBuilderUi((state) => state.rememberPublishJob);
  const validation = useQuery({
    queryKey: ["publisher-draft-validation", draftId],
    queryFn: () => publisherApi.validateDraft(draftId),
  });
  const job = useQuery({
    queryKey: ["publish-job", jobId],
    queryFn: () => publisherApi.getPublishJob(jobId!),
    enabled: jobId !== null,
    refetchInterval: (query) => (query.state.data && finishedPublishStages.has(query.state.data.stage) ? false : 1500),
  });
  const start = useMutation({
    mutationFn: () => publisherApi.startPublishPreparation(draftId),
    onSuccess: (started) => {
      queryClient.setQueryData(["publish-job", started.jobId], started);
      rememberPublishJob(draftId, started.jobId);
    },
  });
  const cancel = useMutation({ mutationFn: (id: string) => publisherApi.cancelPublishJob(id) });

  useEffect(() => {
    if (!jobId) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void publisherApi
      .onPublishJob((event) => {
        if (event.jobId !== jobId) return;
        queryClient.setQueryData<PublishJob | null>(["publish-job", jobId], (current) => (current && current.sequence >= event.sequence ? current : event));
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
  const running = current !== null && !finishedPublishStages.has(current.stage);
  const blocked = (validation.data?.length ?? 0) > 0;
  const startError = start.isError ? (asCommandError(start.error)?.message ?? "Preparation could not start.") : null;

  return (
    <section aria-labelledby="publish-heading" className="h-fit rounded-3xl border border-white/10 bg-black/20 p-6">
      <div className="flex items-center justify-between gap-3">
        <h2 id="publish-heading" className="font-semibold">Prepare release</h2>
        {running ? (
          <button type="button" onClick={() => cancel.mutate(current.jobId)} disabled={cancel.isPending} className="rounded-lg bg-white/10 px-2.5 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
            Cancel
          </button>
        ) : (
          <button
            type="button"
            onClick={() => start.mutate()}
            disabled={blocked || start.isPending || validation.isPending}
            className="flex items-center gap-1.5 rounded-lg bg-amber-300 px-2.5 py-1 text-xs font-semibold text-slate-950 disabled:opacity-40"
          >
            <PackageCheck size={14} /> {current ? "Prepare again" : "Prepare release"}
          </button>
        )}
      </div>
      {blocked && !running ? <p className="mt-3 text-xs text-slate-500">Resolve the blocking issues above first.</p> : null}
      {startError ? <p role="alert" className="mt-3 text-sm text-red-300">{startError}</p> : null}
      {!current ? (
        <p className="mt-4 text-sm text-slate-500">Copies the files into a new package, writes course.json, and builds a verified torrent. Draft sources are only read.</p>
      ) : (
        <JobView job={current} draftRevision={revision} />
      )}
    </section>
  );
}

function JobView({ job, draftRevision }: { job: PublishJob; draftRevision: number }) {
  const reached = stageOrder.indexOf(job.stage);
  const stopped = job.stage === "FAILED" || job.stage === "CANCELLED";
  const percent = job.progress && job.progress.totalBytes > 0 ? Math.floor((job.progress.completedBytes / job.progress.totalBytes) * 100) : null;
  return (
    <div className="mt-4 space-y-4 text-sm">
      <ol aria-label="Preparation steps" className="space-y-2">
        {steps.map((step, index) => {
          const done = job.stage === "READY_TO_PUBLISH" || (reached > index && !stopped);
          const active = step.stage === job.stage && !finishedPublishStages.has(job.stage);
          return (
            <li key={step.stage} aria-current={active ? "step" : undefined} className={`flex items-center gap-2 ${done || active ? "text-slate-200" : "text-slate-500"}`}>
              {done ? <CheckCircle2 size={15} className="text-emerald-300" /> : active ? <Loader2 size={15} className="animate-spin text-amber-300" /> : <CircleDashed size={15} />}
              {step.label}
              {active && percent !== null ? <span className="ml-auto text-xs text-slate-400">{percent}%</span> : null}
            </li>
          );
        })}
      </ol>
      {percent !== null && !finishedPublishStages.has(job.stage) ? (
        <div role="progressbar" aria-label="Current step progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} className="h-1.5 overflow-hidden rounded-full bg-white/10">
          <div className="h-full bg-amber-300" style={{ width: `${percent}%` }} />
        </div>
      ) : null}
      {job.stage === "CANCELLED" ? <p className="text-slate-400">Cancelled. Partial files were removed.</p> : null}
      {job.failure ? (
        <p role="alert" className="flex gap-2 text-red-200"><XCircle size={16} className="mt-0.5 shrink-0" />{job.failure.message}</p>
      ) : null}
      {job.prepared ? (
        <div className="space-y-3 rounded-2xl border border-emerald-300/20 bg-emerald-300/5 p-4">
          <p className="text-emerald-200">Verified and ready. Activation into an immutable release comes next.</p>
          {job.draftRevision !== draftRevision ? <p className="text-xs text-amber-200">Prepared from r{job.draftRevision}; the draft has changed since.</p> : null}
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
            <dt className="text-slate-500">Version</dt><dd>{job.prepared.courseVersion}</dd>
            <dt className="text-slate-500">Files</dt><dd>{job.prepared.fileCount} ({formatBytes(job.prepared.totalBytes)})</dd>
            <dt className="text-slate-500">Pieces</dt><dd>{job.prepared.pieceCount} × {formatBytes(job.prepared.pieceLength)}</dd>
            <dt className="text-slate-500">Info hash</dt><dd className="break-all font-mono">{job.prepared.infoHash}</dd>
          </dl>
          <label className="block text-xs text-slate-500">
            Magnet link
            <input readOnly value={job.prepared.magnet} onFocus={(event) => event.currentTarget.select()} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2 py-1 font-mono text-slate-200" />
          </label>
        </div>
      ) : null}
    </div>
  );
}
