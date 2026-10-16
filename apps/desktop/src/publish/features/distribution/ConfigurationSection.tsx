import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FileDown, FileUp } from "lucide-react";
import { useState } from "react";

import { asCommandError, publisherApi, type ConfigurationPreview, type NetworkConfiguration } from "../../services/publisher";

const profileLabels: Record<NetworkConfiguration["profile"], string> = {
  local_network: "Local network",
  internet: "Internet",
  loopback: "This computer only",
};

/** One line describing imported network settings. */
export function describeNetwork(network: NetworkConfiguration): string {
  const port = network.peerPort === null ? "a new port each start" : `port ${network.peerPort}`;
  const forwarding = network.portForwarding ? "router forwarding on" : "router forwarding off";
  const limit = (value: number | null) => (value === null ? "unlimited" : `${value} KiB/s`);
  return `${profileLabels[network.profile]}, ${port}, ${forwarding}, download ${limit(network.downloadLimitKib)}, upload ${limit(network.uploadLimitKib)}`;
}

/** Export and import trackers, tracker lists, and optionally the Network settings. */
export function ConfigurationSection() {
  const queryClient = useQueryClient();
  const [includeNetwork, setIncludeNetwork] = useState(true);
  const [preview, setPreview] = useState<ConfigurationPreview | null>(null);
  const [applyNetwork, setApplyNetwork] = useState(true);
  const exportConfiguration = useMutation({ mutationFn: () => publisherApi.exportPublisherConfiguration(includeNetwork) });
  const choose = useMutation({
    mutationFn: publisherApi.choosePublisherConfiguration,
    onSuccess: (chosen) => {
      if (preview && chosen) void publisherApi.discardPublisherConfiguration(preview.token);
      if (chosen) {
        setPreview(chosen);
        setApplyNetwork(chosen.network !== null);
        apply.reset();
      }
    },
  });
  const apply = useMutation({
    mutationFn: (chosen: ConfigurationPreview) => publisherApi.applyPublisherConfiguration(chosen.token, applyNetwork && chosen.network !== null),
    onSuccess: () => {
      setPreview(null);
      void queryClient.invalidateQueries({ queryKey: ["distribution"] });
      void queryClient.invalidateQueries({ queryKey: ["network-settings"] });
    },
  });
  const dismiss = () => {
    if (preview) void publisherApi.discardPublisherConfiguration(preview.token);
    setPreview(null);
  };

  return (
    <section aria-labelledby="configuration-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
      <h2 id="configuration-heading" className="font-semibold">Publisher configuration</h2>
      <p className="mt-1 text-sm text-slate-400">
        Move your trackers, followed tracker lists, and Network settings to another computer. Private tracker links can contain passkeys, so keep the file private.
      </p>
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-xs text-slate-300">
          <input type="checkbox" checked={includeNetwork} onChange={(event) => setIncludeNetwork(event.target.checked)} />
          Include Network settings
        </label>
        <button type="button" onClick={() => exportConfiguration.mutate()} disabled={exportConfiguration.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
          <FileDown size={14} /> {exportConfiguration.isPending ? "Exporting…" : "Export…"}
        </button>
        <button type="button" onClick={() => choose.mutate()} disabled={choose.isPending || apply.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-50">
          <FileUp size={14} /> {choose.isPending ? "Reading…" : "Import…"}
        </button>
      </div>
      {exportConfiguration.isSuccess && exportConfiguration.data ? <p role="status" className="mt-2 text-xs text-emerald-300">Configuration saved.</p> : null}
      {exportConfiguration.isError ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(exportConfiguration.error)?.message ?? "The configuration could not be exported."}</p> : null}
      {choose.isError ? <p role="alert" className="mt-2 text-xs text-red-300">This file cannot be imported: {asCommandError(choose.error)?.message ?? "it could not be read."}</p> : null}
      {apply.isSuccess ? (
        <p role="status" className="mt-2 text-xs text-emerald-300">
          Imported {apply.data.trackers} trackers and {apply.data.trackerLists} tracker lists{apply.data.networkApplied ? " and the Network settings" : ""}.
          {apply.data.restartRequired ? " Restart Zeron to use the new network profile and port." : ""}
        </p>
      ) : null}

      {preview ? (
        <div role="group" aria-label="Review imported configuration" className="mt-4 space-y-3 rounded-2xl border border-amber-300/30 bg-amber-300/5 p-4 text-xs">
          <p className="text-amber-100">Applying replaces your trackers and followed tracker lists. Releases already published keep the trackers they carry.</p>
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1">
            <dt className="text-slate-500">Trackers</dt>
            <dd className="break-all text-slate-300">{preview.trackers.length ? preview.trackers.join(", ") : "None"}</dd>
            <dt className="text-slate-500">Tracker lists</dt>
            <dd className="break-all text-slate-300">{preview.trackerLists.length ? preview.trackerLists.join(", ") : "None"}</dd>
            <dt className="text-slate-500">Network</dt>
            <dd className="text-slate-300">{preview.network ? describeNetwork(preview.network) : "Not included"}</dd>
          </dl>
          {preview.network ? (
            <label className="flex items-center gap-2 text-slate-300">
              <input type="checkbox" checked={applyNetwork} onChange={(event) => setApplyNetwork(event.target.checked)} />
              Also apply these Network settings
            </label>
          ) : null}
          <div className="flex gap-2">
            <button type="button" onClick={() => apply.mutate(preview)} disabled={apply.isPending} className="rounded-lg bg-amber-300 px-3 py-1.5 font-semibold text-slate-950 disabled:opacity-50">
              {apply.isPending ? "Applying…" : "Apply configuration"}
            </button>
            <button type="button" onClick={dismiss} disabled={apply.isPending} className="rounded-lg bg-white/10 px-3 py-1.5">Cancel</button>
          </div>
          {apply.isError ? <p role="alert" className="text-red-300">{asCommandError(apply.error)?.message ?? "The configuration could not be applied."}</p> : null}
        </div>
      ) : null}
    </section>
  );
}
