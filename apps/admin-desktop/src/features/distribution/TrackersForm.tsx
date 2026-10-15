import { useState, type FormEvent } from "react";

/** One tracker URL per line; blank lines are ignored. */
export function TrackersForm({
  trackers,
  saving,
  error,
  onSave,
}: {
  trackers: string[];
  saving: boolean;
  error: string | null;
  onSave: (trackers: string[]) => void;
}) {
  const [text, setText] = useState(trackers.join("\n"));
  const lines = text.split("\n").map((line) => line.trim()).filter((line) => line !== "");
  const changed = lines.join("\n") !== trackers.join("\n");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSave(lines);
  };

  return (
    <form onSubmit={submit} aria-labelledby="trackers-heading" className="mt-6 border-t border-white/10 pt-5">
      <h3 id="trackers-heading" className="text-sm font-semibold">Trackers</h3>
      <p className="mt-1 text-xs text-slate-400">
        Written into releases you publish from now on; published releases keep theirs. Learners find your seeder faster through a tracker than through the DHT alone.
        Only the Internet profile announces to trackers.
      </p>
      <p className="mt-1 text-xs text-amber-200/80">
        A public tracker shows the IP address of everyone downloading a release to anyone who has its magnet link.
      </p>
      <label htmlFor="trackers" className="mt-4 block text-xs font-medium text-slate-300">Tracker URLs, one per line</label>
      <textarea
        id="trackers"
        rows={4}
        spellCheck={false}
        placeholder="udp://tracker.example.org:6969/announce"
        value={text}
        onChange={(event) => setText(event.target.value)}
        aria-invalid={error !== null}
        aria-describedby={error ? "trackers-error" : undefined}
        className="mt-1 w-full rounded-lg border border-white/15 bg-black/20 px-3 py-1.5 font-mono text-xs"
      />
      {error ? <p id="trackers-error" role="alert" className="mt-1 text-xs text-red-300">{error}</p> : null}
      <button
        type="submit"
        disabled={!changed || saving}
        className="mt-3 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40"
      >
        Save trackers
      </button>
    </form>
  );
}
