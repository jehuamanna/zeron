import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft } from "lucide-react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type NetworkProfile } from "../../services/library";

const profiles: Array<{ value: NetworkProfile; label: string; description: string }> = [
  {
    value: "local_network",
    label: "Local network",
    description: "Zeron finds courses shared on your local network. Nothing is announced to the internet.",
  },
  {
    value: "internet",
    label: "Internet",
    description:
      "Also uses the public DHT so Zeron can find courses shared anywhere. Your IP address becomes visible to other DHT users, and courses you share become reachable from the internet.",
  },
  {
    value: "loopback",
    label: "This computer only",
    description: "For testing: only programs on this computer can connect.",
  },
];

export function NetworkSettings({ onBack }: { onBack: () => void }) {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: ["network-settings"], queryFn: libraryApi.getNetworkSettings });
  const save = useMutation({
    mutationFn: (profile: NetworkProfile) => libraryApi.setNetworkProfile(profile),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["network-settings"] }),
  });

  const data = settings.data;
  const restartRequired = data !== undefined && data.configuredProfile !== data.activeProfile;
  const error = save.isError ? (asCommandError(save.error)?.message ?? "The network setting could not be saved.") : null;

  return (
    <div className="max-w-2xl">
      <Button variant="ghost" onClick={onBack} className="mb-7">
        <ChevronLeft size={17} /> Back
      </Button>
      <section aria-labelledby="network-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h1 id="network-heading" className="text-2xl font-semibold tracking-tight">Network</h1>
        <p className="mt-1 text-sm text-slate-400">Where Zeron looks for torrent courses, and who can reach the courses you share.</p>
        {settings.isPending ? (
          <p className="mt-5 text-sm text-slate-400">Loading network settings…</p>
        ) : settings.isError || !data ? (
          <p role="alert" className="mt-5 text-sm text-red-300">Network settings are unavailable.</p>
        ) : (
          <>
            <fieldset className="mt-5 space-y-3">
              <legend className="sr-only">Network profile</legend>
              {profiles.map((profile) => (
                <label key={profile.value} className="flex cursor-pointer gap-3 rounded-xl p-2 hover:bg-white/[0.03]">
                  <input
                    type="radio"
                    name="network-profile"
                    value={profile.value}
                    checked={data.configuredProfile === profile.value}
                    disabled={save.isPending}
                    onChange={() => save.mutate(profile.value)}
                    className="mt-1 accent-cyan-300"
                  />
                  <span>
                    <span className="block text-sm font-medium">{profile.label}</span>
                    <span className="block text-xs text-slate-400">{profile.description}</span>
                  </span>
                </label>
              ))}
            </fieldset>
            {restartRequired ? (
              <p role="status" className="mt-4 text-xs text-amber-200">Restart Zeron to apply the new network setting.</p>
            ) : data.torrentEngine === "unavailable" ? (
              <p className="mt-4 text-xs text-amber-200">The torrent engine did not start, so torrent courses are unavailable until Zeron restarts.</p>
            ) : (
              <p className="mt-4 text-xs text-slate-500">
                Active now
                {data.downloadPort ? ` · downloads use port ${data.downloadPort}` : ""}
                {data.sharingPort ? ` · shared courses are served on port ${data.sharingPort}` : ""}. UPnP port mapping is never used.
              </p>
            )}
            {error ? <p role="alert" className="mt-3 text-sm text-red-300">{error}</p> : null}
          </>
        )}
      </section>
    </div>
  );
}
