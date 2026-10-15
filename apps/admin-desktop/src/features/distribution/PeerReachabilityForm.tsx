import { useState, type FormEvent } from "react";

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
      <h3 id="reachability-heading" className="text-sm font-semibold">Reaching learners</h3>
      <p className="mt-1 text-xs text-slate-400">
        Behind a router, learners on the internet can connect only if this port is forwarded to this computer. A fixed port lets you forward it yourself.
      </p>
      <label htmlFor="peer-port" className="mt-4 block text-xs font-medium text-slate-300">Peer port</label>
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
          className="mt-1 accent-amber-300"
        />
        <span>
          <span className="block text-sm font-medium">Ask my router to forward the port (UPnP)</span>
          <span className="block text-xs text-slate-400">
            {internet
              ? "While Zeron Publisher runs, it asks your router to forward the port. Any device on your network can make the same request, and some routers have UPnP turned off."
              : "Available with the Internet profile."}
          </span>
        </span>
      </label>
      <button
        type="submit"
        disabled={!changed || saving}
        className="mt-4 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40"
      >
        Save
      </button>
    </form>
  );
}
