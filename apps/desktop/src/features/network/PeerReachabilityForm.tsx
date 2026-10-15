import { useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";

const MIN_PORT = 1024;
const MAX_PORT = 65535;

/** Parse the port field: empty means a new port on each start. */
export function parsePeerPort(value: string): { port: number | null } | { error: string } {
  const trimmed = value.trim();
  if (trimmed === "") return { port: null };
  const port = /^\d+$/.test(trimmed) ? Number(trimmed) : NaN;
  return port >= MIN_PORT && port <= MAX_PORT ? { port } : { error: `Choose a port from ${MIN_PORT} to ${MAX_PORT}, or leave it empty.` };
}

export function PeerReachabilityForm({
  peerPort,
  portForwarding,
  internet,
  saving,
  onSave,
}: {
  peerPort: number | null;
  portForwarding: boolean;
  /** Router forwarding applies only to the Internet profile. */
  internet: boolean;
  saving: boolean;
  onSave: (peerPort: number | null, portForwarding: boolean) => void;
}) {
  const [port, setPort] = useState(peerPort?.toString() ?? "");
  const [forwarding, setForwarding] = useState(portForwarding);
  const [error, setError] = useState<string | null>(null);
  const changed = port.trim() !== (peerPort?.toString() ?? "") || forwarding !== portForwarding;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const parsed = parsePeerPort(port);
    if ("error" in parsed) {
      setError(parsed.error);
      return;
    }
    setError(null);
    onSave(parsed.port, forwarding);
  };

  return (
    <form onSubmit={submit} aria-labelledby="reachability-heading" className="mt-6 border-t border-white/10 pt-5">
      <h2 id="reachability-heading" className="text-sm font-semibold">Reaching people who download from you</h2>
      <p className="mt-1 text-xs text-slate-400">
        Behind a home router, other computers can connect to you only if the router forwards a port to this computer. The fixed port applies to the courses you share and the releases you seed.
      </p>
      <label htmlFor="peer-port" className="mt-4 block text-xs font-medium text-slate-300">Port for sharing and seeding</label>
      <input
        id="peer-port"
        inputMode="numeric"
        placeholder="New port each start"
        value={port}
        onChange={(event) => setPort(event.target.value)}
        aria-invalid={error !== null}
        aria-describedby={error ? "peer-port-error" : undefined}
        className="mt-1 w-48 rounded-lg border border-white/15 bg-black/20 px-3 py-1.5 text-sm"
      />
      {error ? <p id="peer-port-error" role="alert" className="mt-1 text-xs text-red-300">{error}</p> : null}
      <label className={`mt-4 flex gap-3 ${internet ? "cursor-pointer" : "opacity-50"}`}>
        <input
          type="checkbox"
          checked={forwarding}
          disabled={!internet}
          onChange={(event) => setForwarding(event.target.checked)}
          className="mt-1 accent-cyan-300"
        />
        <span>
          <span className="block text-sm font-medium">Ask my router to open Zeron's ports (UPnP, NAT-PMP, PCP)</span>
          <span className="block text-xs text-slate-400">
            {internet
              ? "On by default, like most BitTorrent apps. It lets seeders behind their own routers reach you to deliver a course, and others download what you share or publish. While Zeron runs, it asks your router to open the ports for TCP and UDP, and closes them when Zeron quits. Any device on your network can make the same request, and some routers have these protocols turned off."
              : "Available with the Internet profile."}
          </span>
        </span>
      </label>
      <Button type="submit" disabled={!changed || saving} className="mt-4">Save</Button>
    </form>
  );
}
