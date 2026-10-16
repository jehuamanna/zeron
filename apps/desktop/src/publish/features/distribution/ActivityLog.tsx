import { useMutation, useQuery } from "@tanstack/react-query";
import { Download, History } from "lucide-react";

import { asCommandError, publisherApi, type ActivityEvent, type ActivityKind } from "../../services/publisher";

const kindLabels: Record<ActivityKind, string> = {
  release_published: "Published",
  release_made_current: "Made current",
  release_archived: "Archived",
  release_files_deleted: "Files deleted",
  seeding_started: "Seeding started",
  seeding_stopped: "Seeding stopped",
  seeding_failed: "Seeding failed",
  tracker_lists_refreshed: "Tracker lists",
  tracker_list_failed: "Tracker list failed",
  course_deleted: "Course deleted",
};

const failureKinds: ReadonlySet<ActivityKind> = new Set(["seeding_failed", "tracker_list_failed"]);

/**
 * Distribution events, newest first. With a `releaseId` it shows that release's
 * recent events; without one, every event with an export of the redacted log.
 */
export function ActivityLog({ releaseId = null, limit = 50 }: { releaseId?: string | null; limit?: number }) {
  const activity = useQuery({
    queryKey: ["distribution-activity", releaseId],
    queryFn: () => publisherApi.getDistributionActivity(releaseId),
    refetchInterval: 5000,
  });
  const exportLog = useMutation({ mutationFn: publisherApi.exportDistributionActivity });
  const scoped = releaseId !== null;
  const heading = scoped ? "Recent activity" : "Activity";
  const headingId = scoped ? `activity-${releaseId}` : "activity-heading";

  return (
    <section aria-labelledby={headingId} className={scoped ? "" : "rounded-3xl border border-white/10 bg-white/[0.035] p-6"}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 id={headingId} className={scoped ? "text-xs font-semibold text-slate-400" : "flex items-center gap-2 font-semibold"}>
          {scoped ? null : <History size={16} className="text-slate-400" />} {heading}
        </h3>
        {scoped ? null : (
          <button type="button" onClick={() => exportLog.mutate()} disabled={exportLog.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
            <Download size={14} /> {exportLog.isPending ? "Exporting…" : "Export log…"}
          </button>
        )}
      </div>
      {scoped ? null : <p className="mt-1 text-sm text-slate-500">Kept on this computer only. File paths, network addresses, and private parts of links are removed before anything is recorded.</p>}
      {exportLog.isSuccess && exportLog.data ? <p role="status" className="mt-2 text-xs text-emerald-300">Activity log saved.</p> : null}
      {exportLog.isError ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(exportLog.error)?.message ?? "The activity log could not be exported."}</p> : null}
      {activity.isPending ? (
        <p className="mt-2 text-xs text-slate-500">Loading activity…</p>
      ) : activity.isError ? (
        <p role="alert" className="mt-2 text-xs text-red-300">Activity is unavailable.</p>
      ) : activity.data.length === 0 ? (
        <p className="mt-2 text-xs text-slate-500">Nothing has happened yet.</p>
      ) : (
        <ol aria-label={heading} className="mt-3 max-h-72 space-y-1.5 overflow-y-auto text-xs">
          {activity.data.slice(0, limit).map((event) => <EventRow key={event.eventId} event={event} showVersion={!scoped} />)}
        </ol>
      )}
    </section>
  );
}

function EventRow({ event, showVersion }: { event: ActivityEvent; showVersion: boolean }) {
  const failed = failureKinds.has(event.kind);
  return (
    <li className="grid grid-cols-[max-content_max-content_minmax(0,1fr)] gap-x-3">
      <time dateTime={new Date(event.atUnixSeconds * 1000).toISOString()} className="text-slate-500">
        {new Date(event.atUnixSeconds * 1000).toLocaleString()}
      </time>
      <span className={failed ? "font-semibold text-red-200" : "font-semibold text-slate-300"}>
        {kindLabels[event.kind] ?? event.kind}
        {showVersion && event.courseVersion ? ` · v${event.courseVersion}` : ""}
      </span>
      <span className="break-words text-slate-400">{event.detail}</span>
    </li>
  );
}
