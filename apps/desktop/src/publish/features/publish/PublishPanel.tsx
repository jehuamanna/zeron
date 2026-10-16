import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, CircleDashed, Loader2, PackageCheck, RotateCcw, Rocket, Undo2, XCircle } from "lucide-react";
import { useEffect, useState } from "react";

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
  { stage: "READY_TO_PUBLISH", label: "Ready to publish" },
  { stage: "PUBLISHED", label: "Published" },
];

const stageOrder: PublishStage[] = steps.map((step) => step.stage);

/** What the publisher can do about a failed preparation, by failure code. */
export function failureAdvice(code: string): string {
  switch (code) {
    case "draft_invalid":
      return "Fix the issues listed above, then retry.";
    case "manifest_invalid":
      return "The course details could not be written as a valid course.json. Check the title, version, and tags, then retry.";
    case "source_changed":
      return "A source file changed or disappeared while it was being copied. Check the lesson files, then retry.";
    case "limit_exceeded":
      return "The course is over a package limit. Remove files or split the course, then retry.";
    case "verification_failed":
      return "The independent check did not match the files just written. Retry; if it fails again, check the disk.";
    default:
      return "Partial files were removed. Retry when ready.";
  }
}

/** What the publisher can do when publishing a prepared release fails, by error code. */
export function activationAdvice(code: string | undefined): string | null {
  switch (code) {
    case "version_already_released":
      return "Discard this preparation, change the version in the draft, and prepare again.";
    case "release_activation_failed":
      return "Nothing was published. Discard this preparation and prepare again.";
    default:
      return null;
  }
}

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
            {current?.stage === "FAILED" ? <RotateCcw size={14} /> : <PackageCheck size={14} />} {current?.stage === "FAILED" ? "Retry" : current ? "Prepare again" : "Prepare release"}
          </button>
        )}
      </div>
      {blocked && !running ? <p className="mt-3 text-xs text-slate-500">Resolve the blocking issues above first.</p> : null}
      {startError ? <p role="alert" className="mt-3 text-sm text-red-300">{startError}</p> : null}
      {!current ? (
        <p className="mt-4 text-sm text-slate-500">Copies the files into a new package, writes course.json, and builds a verified torrent. Draft sources are only read.</p>
      ) : (
        <JobView job={current} draftRevision={revision} onPublished={() => void queryClient.invalidateQueries({ queryKey: ["publisher-releases"] })} />
      )}
    </section>
  );
}

function JobView({ job, draftRevision, onPublished }: { job: PublishJob; draftRevision: number; onPublished: () => void }) {
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const discard = useMutation({
    mutationFn: () => publisherApi.discardPublishJob(job.jobId),
    onSuccess: (discarded) => {
      setConfirming(false);
      queryClient.setQueryData<PublishJob | null>(["publish-job", job.jobId], (current) => (current && current.sequence >= discarded.sequence ? current : discarded));
    },
  });
  const activate = useMutation({
    mutationFn: () => publisherApi.activateRelease(job.jobId),
    onSuccess: async () => {
      setConfirming(false);
      onPublished();
      await queryClient.invalidateQueries({ queryKey: ["publish-job", job.jobId] });
    },
  });
  const reached = stageOrder.indexOf(job.stage);
  const settled = job.stage === "READY_TO_PUBLISH" || job.stage === "PUBLISHED";
  const percent = job.progress && job.progress.totalBytes > 0 ? Math.floor((job.progress.completedBytes / job.progress.totalBytes) * 100) : null;
  const activationError = activate.isError ? (asCommandError(activate.error)?.message ?? "The release could not be published.") : null;
  const activationHelp = activate.isError ? activationAdvice(asCommandError(activate.error)?.code) : null;
  const discardError = discard.isError ? (asCommandError(discard.error)?.message ?? "The preparation could not be discarded.") : null;
  return (
    <div className="mt-4 space-y-4 text-sm">
      <ol aria-label="Preparation steps" className="space-y-2">
        {steps.map((step, index) => {
          const done = reached > index || (reached === index && settled);
          const active = reached === index && !finishedPublishStages.has(job.stage);
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
      {job.stage === "CANCELLED" ? <p className="text-slate-400">Cancelled. Partial files were removed. The draft can be prepared again.</p> : null}
      {job.failure ? (
        <div className="space-y-1">
          <p role="alert" className="flex gap-2 text-red-200"><XCircle size={16} className="mt-0.5 shrink-0" />{job.failure.message}</p>
          <p className="pl-6 text-xs text-slate-400">{failureAdvice(job.failure.code)}</p>
        </div>
      ) : null}
      {job.prepared ? (
        <div className="space-y-3 rounded-2xl border border-emerald-300/20 bg-emerald-300/5 p-4">
          <p className="text-emerald-200">
            {job.stage === "PUBLISHED" ? `Published version ${job.prepared.courseVersion}. Its files can no longer change.` : "Verified and ready to publish."}
          </p>
          {job.draftRevision !== draftRevision ? <p className="text-xs text-amber-200">Prepared from r{job.draftRevision}; the draft has changed since.</p> : null}
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
            <dt className="text-slate-500">Version</dt><dd>{job.prepared.courseVersion}</dd>
            <dt className="text-slate-500">Files</dt><dd>{job.prepared.fileCount} ({formatBytes(job.prepared.totalBytes)})</dd>
            <dt className="text-slate-500">Pieces</dt><dd>{job.prepared.pieceCount} × {formatBytes(job.prepared.pieceLength)}</dd>
            <dt className="text-slate-500">Info hash</dt><dd className="break-all font-mono">{job.prepared.infoHash}</dd>
            <dt className="text-slate-500">Signature</dt><dd>{job.prepared.signed ? "Signed with your publisher identity" : "Unsigned"}</dd>
            <dt className="text-slate-500">Trackers</dt>
            <dd>{job.prepared.trackerCount ? job.prepared.trackerCount : "None: learners find it through local discovery or the DHT"}</dd>
          </dl>
          <label className="block text-xs text-slate-500">
            Magnet link
            <input readOnly value={job.prepared.magnet} onFocus={(event) => event.currentTarget.select()} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2 py-1 font-mono text-slate-200" />
          </label>
          {job.stage === "READY_TO_PUBLISH" ? (
            confirming ? (
              <div role="group" aria-label="Confirm publishing" className="space-y-2 rounded-xl border border-amber-300/30 bg-amber-300/10 p-3 text-xs text-amber-100">
                <p>Publishing makes version {job.prepared.courseVersion} permanent. Its files and torrent can never be changed, and this version number cannot be reused.</p>
                <div className="flex gap-2">
                  <button type="button" onClick={() => activate.mutate()} disabled={activate.isPending} className="rounded-lg bg-amber-300 px-2.5 py-1 font-semibold text-slate-950 disabled:opacity-50">
                    {activate.isPending ? "Publishing…" : "Publish permanently"}
                  </button>
                  <button type="button" autoFocus onClick={() => setConfirming(false)} disabled={activate.isPending} className="rounded-lg bg-white/10 px-2.5 py-1">
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap gap-2">
                <button type="button" onClick={() => setConfirming(true)} disabled={discard.isPending} className="flex items-center gap-1.5 rounded-lg bg-emerald-300 px-3 py-1.5 text-xs font-semibold text-slate-950 disabled:opacity-50">
                  <Rocket size={14} /> Publish release
                </button>
                <button type="button" onClick={() => discard.mutate()} disabled={discard.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
                  <Undo2 size={14} /> {discard.isPending ? "Discarding…" : "Discard"}
                </button>
              </div>
            )
          ) : null}
          {activationError ? <p role="alert" className="text-red-200">{activationError}</p> : null}
          {activationHelp ? <p className="text-xs text-slate-400">{activationHelp}</p> : null}
          {discardError ? <p role="alert" className="text-red-200">{discardError}</p> : null}
        </div>
      ) : null}
    </div>
  );
}
