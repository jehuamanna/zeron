import { useState, type FormEvent } from "react";

const linesOf = (text: string) => text.split("\n").map((line) => line.trim()).filter((line) => line !== "");

/** A textarea of URLs, one per line, saved as a list. */
function UrlListForm({
  id,
  label,
  placeholder,
  saveLabel,
  urls,
  saving,
  error,
  onSave,
}: {
  id: string;
  label: string;
  placeholder: string;
  saveLabel: string;
  urls: string[];
  saving: boolean;
  error: string | null;
  onSave: (urls: string[]) => void;
}) {
  const [text, setText] = useState(urls.join("\n"));
  const changed = linesOf(text).join("\n") !== urls.join("\n");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSave(linesOf(text));
  };
  return (
    <form onSubmit={submit} className="mt-4">
      <label htmlFor={id} className="block text-xs font-medium text-slate-300">{label}</label>
      <textarea
        id={id}
        rows={3}
        spellCheck={false}
        placeholder={placeholder}
        value={text}
        onChange={(event) => setText(event.target.value)}
        aria-invalid={error !== null}
        aria-describedby={error ? `${id}-error` : undefined}
        className="mt-1 w-full rounded-lg border border-white/15 bg-black/20 px-3 py-1.5 font-mono text-xs"
      />
      {error ? <p id={`${id}-error`} role="alert" className="mt-1 text-xs text-red-300">{error}</p> : null}
      <button
        type="submit"
        disabled={!changed || saving}
        className="mt-2 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40"
      >
        {saveLabel}
      </button>
    </form>
  );
}

export function TrackerListsForm(props: { urls: string[]; saving: boolean; error: string | null; onSave: (urls: string[]) => void }) {
  return (
    <UrlListForm
      id="tracker-lists"
      label="Tracker list URLs, one per line"
      placeholder="https://example.org/trackers.txt"
      saveLabel="Save lists"
      {...props}
    />
  );
}

export function TrackersForm({ trackers, ...props }: { trackers: string[]; saving: boolean; error: string | null; onSave: (trackers: string[]) => void }) {
  return (
    <UrlListForm
      id="trackers"
      label="Additional tracker URLs, one per line"
      placeholder="udp://tracker.example.org:6969/announce"
      saveLabel="Save trackers"
      urls={trackers}
      {...props}
    />
  );
}
