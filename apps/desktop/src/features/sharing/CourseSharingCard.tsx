import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Share2 } from "lucide-react";

import { asCommandError, libraryApi, type CourseSharing } from "../../services/library";

const audience: Record<CourseSharing["networkProfile"], string> = {
  loopback: "on this computer",
  local_network: "on your local network",
  internet: "on your local network or over the internet",
};

const formatBytes = (bytes: number) =>
  new Intl.NumberFormat(undefined, { style: "unit", unit: "kilobyte", maximumFractionDigits: 1 }).format(bytes / 1000);

export function CourseSharingCard({ courseId, compact = false }: {
  courseId: string;
  /** A one-line switch, for the course overview's detail row. */
  compact?: boolean;
}) {
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: ["course-sharing", courseId],
    queryFn: () => libraryApi.getCourseSharing(courseId),
    refetchInterval: (query) => (query.state.data?.sharing ? 2000 : false),
  });
  const toggle = useMutation({
    mutationFn: (sharing: boolean) => libraryApi.setCourseSharing(courseId, sharing),
    onSuccess: async (next) => {
      queryClient.setQueryData(["course-sharing", courseId], next);
      await queryClient.invalidateQueries({ queryKey: ["courses"] });
    },
  });

  const data = status.data ?? null;
  if (status.isPending || !data) return null;
  const on = data.sharing;
  const error = toggle.isError ? (asCommandError(toggle.error)?.message ?? "The sharing choice could not be saved.") : null;

  if (compact) {
    const detail = on
      ? `On: Zeron uploads the parts of this course this computer has verified to other learners ${audience[data.networkProfile]}, who can see this computer's IP address.`
      : "Off: Zeron uploads nothing for this course. Only parts this computer has downloaded can be shared.";
    return (
      <div className="inline-flex items-center gap-2 rounded-full border border-line bg-surface py-1 pl-2.5 pr-1.5 text-xs" title={detail}>
        <Share2 size={13} className="text-muted" />
        <span id="sharing-heading" className="font-medium text-fg-soft">Share with other learners</span>
        {on ? <span className="tabular-nums text-subtle">· {data.connectedPeers} peers · {formatBytes(data.uploadedBytes)} up</span> : null}
        <button
          type="button"
          role="switch"
          aria-checked={on}
          aria-labelledby="sharing-heading"
          disabled={!data.available || toggle.isPending}
          onClick={() => toggle.mutate(!on)}
          className={`relative h-4 w-7 shrink-0 rounded-full transition disabled:opacity-50 ${on ? "bg-accent" : "bg-white/15"}`}
        >
          <span className={`absolute top-0.5 size-3 rounded-full transition-all ${on ? "left-3.5 bg-on-accent" : "left-0.5 bg-surface"}`} />
        </button>
        {error ? <span role="alert" className="text-red-300">{error}</span> : null}
      </div>
    );
  }

  return (
    <section aria-labelledby="sharing-heading" className="mt-5 max-w-2xl rounded-2xl border border-white/10 bg-white/[0.04] p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 id="sharing-heading" className="flex items-center gap-2 font-semibold">
            <Share2 size={16} /> Share with other learners
          </h2>
          <p className="mt-1 text-sm text-slate-400">
            {on
              ? `On. Zeron uploads the parts of this course that this computer has verified to other learners ${audience[data.networkProfile]}. They can see this computer's IP address.`
              : "Off. Zeron uploads nothing for this course."}
          </p>
          <p className="mt-1 text-xs text-slate-500">Only the parts of this course that this computer has downloaded can be shared.</p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={on}
          aria-labelledby="sharing-heading"
          disabled={!data.available || toggle.isPending}
          onClick={() => toggle.mutate(!on)}
          className={`relative h-7 w-12 shrink-0 rounded-full transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-300 disabled:opacity-50 ${on ? "bg-cyan-300" : "bg-white/15"}`}
        >
          <span className={`absolute top-1 size-5 rounded-full bg-slate-950 transition-all ${on ? "left-6" : "left-1"}`} />
        </button>
      </div>
      {!data.available ? (
        <p className="mt-3 text-sm text-amber-200">Sharing is unavailable because its torrent engine could not start.</p>
      ) : null}
      {on ? (
        <>
          <dl className="mt-4 grid grid-cols-3 gap-3 text-sm">
            <div>
              <dt className="text-xs text-slate-500">Connections</dt>
              <dd>{data.connectedPeers}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-500">Uploaded</dt>
              <dd>{formatBytes(data.uploadedBytes)}</dd>
            </div>
            <div>
              <dt className="text-xs text-slate-500">Upload rate</dt>
              <dd>{formatBytes(data.uploadBytesPerSecond)}/s</dd>
            </div>
          </dl>
          <p className="mt-2 text-xs text-slate-500">
            Measured on this computer since sharing started in this session. This is not a count of downloads.
            {data.state === "failed" ? " The shared data no longer matches the torrent, so nothing is being served." : ""}
          </p>
        </>
      ) : null}
      {error ? <p role="alert" className="mt-3 text-sm text-red-300">{error}</p> : null}
    </section>
  );
}
