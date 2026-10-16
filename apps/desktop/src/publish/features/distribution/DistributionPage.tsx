import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CirclePause, CirclePlay, Loader2, Network, Radio } from "lucide-react";

import { formatBytes } from "../../lib/format";
import { asCommandError, publisherApi, type SeedingStatus } from "../../services/publisher";
import { ActivityLog } from "./ActivityLog";
import { TrackersSection } from "./TrackersSection";

/** Release seeding and trackers. Where peers can reach releases is the app's one Network setting. */
export function DistributionPage({ onOpenNetwork }: { onOpenNetwork: () => void }) {
  const queryClient = useQueryClient();
  const distribution = useQuery({ queryKey: ["distribution"], queryFn: publisherApi.getDistribution, refetchInterval: 2000 });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["distribution"] });

  if (distribution.isPending) return <p className="text-slate-400">Loading distribution…</p>;
  if (distribution.isError) return <p role="alert" className="text-red-300">Distribution status is unavailable.</p>;
  const data = distribution.data;

  return (
    <div className="space-y-8">
      {data.engineError ? (
        <p role="alert" className="flex gap-2 rounded-2xl border border-red-400/30 bg-red-400/10 p-4 text-sm text-red-200">
          <AlertTriangle size={16} className="mt-0.5 shrink-0" /> Nothing can be seeded: {data.engineError}
        </p>
      ) : null}

      <section aria-labelledby="reach-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h2 id="reach-heading" className="font-semibold">Reaching learners</h2>
        <p className="mt-1 text-sm text-slate-400">
          Releases seed from the same port as the courses you share. Choose who can reach them, the port, and speed limits in{" "}
          <button type="button" onClick={onOpenNetwork} className="inline-flex items-center gap-1 align-middle font-medium text-amber-300 hover:underline">
            <Network size={14} /> Network settings
          </button>
          .
        </p>
        <TrackersSection data={data} onChanged={refresh} />
      </section>

      <section aria-labelledby="seeding-heading">
        <h2 id="seeding-heading" className="flex items-center gap-2 text-lg font-semibold"><Radio size={17} className="text-slate-400" /> Seeding</h2>
        <p className="mt-1 text-sm text-slate-500">Metrics are local to this computer and this session; they are not download counts.</p>
        {data.releases.length ? (
          <ul aria-label="Seeding releases" className="mt-4 space-y-3">
            {data.releases.map((release) => (
              <ReleaseRow key={release.releaseId} release={release} disabled={data.engineError !== null} onChanged={refresh} />
            ))}
          </ul>
        ) : (
          <p className="mt-4 text-sm text-slate-500">Publish a release to seed it.</p>
        )}
      </section>

      <ActivityLog />
    </div>
  );
}

function ReleaseRow({ release, disabled, onChanged }: { release: SeedingStatus; disabled: boolean; onChanged: () => void }) {
  const toggle = useMutation({
    mutationFn: () => (release.enabled ? publisherApi.stopSeeding(release.releaseId) : publisherApi.startSeeding(release.releaseId)),
    onSettled: onChanged,
  });
  const error = toggle.isError ? (asCommandError(toggle.error)?.message ?? "The change failed.") : null;
  const checking = release.state === "checking";
  const archived = release.releaseStatus === "archived";
  const percent = release.totalBytes > 0 ? Math.floor((release.verifiedBytes / release.totalBytes) * 100) : 0;
  return (
    <li className="rounded-2xl border border-white/10 bg-white/[0.035] px-5 py-4 text-sm">
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
        <div className="min-w-0 flex-1">
          <p className="truncate font-semibold">{release.title}</p>
          <p className="text-xs text-slate-500">Version {release.courseVersion} · {release.releaseStatus} · {formatBytes(release.totalBytes)}</p>
        </div>
        <StateBadge release={release} percent={percent} />
        <button
          type="button"
          onClick={() => toggle.mutate()}
          disabled={disabled || toggle.isPending || (archived && !release.enabled)}
          title={archived && !release.enabled ? "Archived releases are not seeded" : undefined}
          aria-label={`${release.enabled ? "Stop seeding" : "Start seeding"} ${release.title} ${release.courseVersion}`}
          className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40"
        >
          {release.enabled ? <CirclePause size={14} /> : <CirclePlay size={14} />} {release.enabled ? "Stop seeding" : "Start seeding"}
        </button>
      </div>
      {release.state === "seeding" ? (
        <p className="mt-2 text-xs text-slate-400">
          {release.connectedPeers} {release.connectedPeers === 1 ? "peer" : "peers"} connected · {formatBytes(release.uploadBytesPerSecond)}/s up · {formatBytes(release.uploadedBytes)} uploaded this session
        </p>
      ) : null}
      {checking ? <p className="mt-2 text-xs text-slate-400">Verifying release files before serving them…</p> : null}
      {release.failure ? <p role="alert" className="mt-2 text-xs text-red-200">{release.failure}</p> : null}
      {error ? <p role="alert" className="mt-2 text-xs text-red-200">{error}</p> : null}
    </li>
  );
}

function StateBadge({ release, percent }: { release: SeedingStatus; percent: number }) {
  const base = "flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs";
  switch (release.state) {
    case "seeding":
      return <span className={`${base} bg-emerald-300/10 text-emerald-300`}>Seeding</span>;
    case "checking":
      return <span className={`${base} bg-white/10 text-slate-300`}><Loader2 size={12} className="animate-spin" /> Checking {percent}%</span>;
    case "failed":
      return <span className={`${base} bg-red-400/15 text-red-200`}>Not serving</span>;
    default:
      return <span className={`${base} bg-white/5 text-slate-500`}>{release.enabled ? "Starting" : "Not seeding"}</span>;
  }
}
