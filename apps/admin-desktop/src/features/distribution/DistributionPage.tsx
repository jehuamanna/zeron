import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CirclePause, CirclePlay, Loader2, Radio } from "lucide-react";

import { formatBytes } from "../../lib/format";
import { asCommandError, publisherApi, type NetworkProfile, type SeedingStatus } from "../../services/publisher";
import { PeerReachabilityForm } from "./PeerReachabilityForm";
import { TrackersSection } from "./TrackersSection";
import { UploadLimitForm } from "./UploadLimitForm";

const profiles: Array<{ value: NetworkProfile; label: string; description: string }> = [
  {
    value: "local_network",
    label: "Local network",
    description: "Learners on the same network find your releases automatically. Nothing is announced to the internet.",
  },
  {
    value: "internet",
    label: "Internet",
    description:
      "Also uses the public DHT and the trackers below so learners anywhere can find your releases. Your IP address becomes visible to other DHT and tracker users.",
  },
  {
    value: "loopback",
    label: "This computer only",
    description: "For testing: only programs on this computer can connect.",
  },
];

export function DistributionPage() {
  const queryClient = useQueryClient();
  const distribution = useQuery({ queryKey: ["distribution"], queryFn: publisherApi.getDistribution, refetchInterval: 2000 });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["distribution"] });
  const setProfile = useMutation({ mutationFn: (profile: NetworkProfile) => publisherApi.setNetworkProfile(profile), onSuccess: refresh });
  const setReachability = useMutation({
    mutationFn: ({ peerPort, portForwarding }: { peerPort: number | null; portForwarding: boolean }) =>
      publisherApi.setPeerReachability(peerPort, portForwarding),
    onSuccess: refresh,
  });
  const setUploadLimit = useMutation({
    mutationFn: (uploadKib: number | null) => publisherApi.setUploadLimit(uploadKib),
    onSuccess: refresh,
  });

  if (distribution.isPending) return <p className="text-slate-400">Loading distribution…</p>;
  if (distribution.isError) return <p role="alert" className="text-red-300">Distribution status is unavailable.</p>;
  const data = distribution.data;
  const failed = setProfile.isError ? setProfile.error : setReachability.isError ? setReachability.error : setUploadLimit.isError ? setUploadLimit.error : null;
  const saveError = failed ? (asCommandError(failed)?.message ?? "The network setting could not be saved.") : null;

  return (
    <div className="space-y-8">
      {data.engineError ? (
        <p role="alert" className="flex gap-2 rounded-2xl border border-red-400/30 bg-red-400/10 p-4 text-sm text-red-200">
          <AlertTriangle size={16} className="mt-0.5 shrink-0" /> The torrent engine did not start, so nothing can be seeded: {data.engineError}
        </p>
      ) : null}

      <section aria-labelledby="network-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h2 id="network-heading" className="font-semibold">Network</h2>
        <fieldset className="mt-4 space-y-3">
          <legend className="sr-only">Where seeding is reachable</legend>
          {profiles.map((profile) => (
            <label key={profile.value} className="flex cursor-pointer gap-3 rounded-xl p-2 hover:bg-white/[0.03]">
              <input
                type="radio"
                name="network-profile"
                value={profile.value}
                checked={data.configuredProfile === profile.value}
                disabled={setProfile.isPending}
                onChange={() => setProfile.mutate(profile.value)}
                className="mt-1 accent-amber-300"
              />
              <span>
                <span className="block text-sm font-medium">{profile.label}</span>
                <span className="block text-xs text-slate-400">{profile.description}</span>
              </span>
            </label>
          ))}
        </fieldset>
        <PeerReachabilityForm
          key={`${data.peerPort}-${data.portForwarding}`}
          peerPort={data.peerPort}
          portForwarding={data.portForwarding}
          internet={data.configuredProfile === "internet"}
          saving={setReachability.isPending}
          onSave={(peerPort, portForwarding) => setReachability.mutate({ peerPort, portForwarding })}
        />
        {data.restartRequired ? (
          <p role="status" className="mt-3 text-xs text-amber-200">Restart Zeron Publisher to apply the new network setting.</p>
        ) : (
          <p className="mt-3 text-xs text-slate-500">
            Active now{data.listenPort ? ` · peers connect on port ${data.listenPort}` : ""}.{" "}
            {data.activePortForwarding ? "Your router is asked to forward this port with UPnP." : "UPnP is off."}
          </p>
        )}
        {saveError ? <p role="alert" className="mt-3 text-xs text-red-200">{saveError}</p> : null}
        <UploadLimitForm
          key={String(data.uploadLimitKib)}
          uploadKib={data.uploadLimitKib}
          saving={setUploadLimit.isPending}
          onSave={(uploadKib) => setUploadLimit.mutate(uploadKib)}
        />
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
