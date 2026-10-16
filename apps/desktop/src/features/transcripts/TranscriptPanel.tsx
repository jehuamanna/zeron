import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { asCommandError, libraryApi, type TranscriptCue } from "../../services/library";

/** How often the panel follows the player. */
export const TRANSCRIPT_FOLLOW_MS = 250;

/** The index of the cue playing at `time`, or -1. */
export function activeCueIndex(cues: TranscriptCue[], time: number): number {
  let low = 0;
  let high = cues.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (cues[middle]!.start <= time) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found >= 0 && time < cues[found]!.end ? found : -1;
}

function clock(seconds: number) {
  const whole = Math.floor(seconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, "0");
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

/** The playing lesson's transcript, following the player; a line seeks to it. */
export function TranscriptPanel({ courseId, mediaResourceId, currentTime, onSeek, missing = null }: {
  courseId: string;
  mediaResourceId: string;
  currentTime: () => number;
  onSeek: (seconds: number) => void;
  /** Shown when the lesson has no transcript. */
  missing?: ReactNode;
}) {
  const transcript = useQuery({
    queryKey: ["transcript", courseId, mediaResourceId],
    queryFn: () => libraryApi.transcript(courseId, mediaResourceId),
    staleTime: Infinity,
  });
  const [time, setTime] = useState(0);
  const [filter, setFilter] = useState("");
  const [follow, setFollow] = useState(true);
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    const timer = setInterval(() => setTime(currentTime()), TRANSCRIPT_FOLLOW_MS);
    return () => clearInterval(timer);
  }, [currentTime]);

  const cues = transcript.data?.cues ?? [];
  const active = activeCueIndex(cues, time);
  const query = filter.trim().toLowerCase();
  const shown = useMemo(
    () => cues.map((cue, index) => ({ cue, index })).filter(({ cue }) => !query || cue.text.toLowerCase().includes(query)),
    [cues, query],
  );

  useEffect(() => {
    if (!follow || query || active < 0 || !list.current) return;
    const item = list.current.querySelector<HTMLElement>(`[data-cue="${active}"]`);
    if (!item) return;
    // Scroll the list only, never the page around it.
    const top = item.offsetTop - list.current.offsetTop;
    if (top < list.current.scrollTop || top + item.offsetHeight > list.current.scrollTop + list.current.clientHeight) {
      list.current.scrollTop = Math.max(0, top - list.current.clientHeight / 3);
    }
  }, [active, follow, query]);

  if (transcript.isPending) return null;
  if (transcript.data === null) return <>{missing}</>;
  if (transcript.isError) {
    return <p role="alert" className="p-4 text-sm text-red-300">{asCommandError(transcript.error)?.message ?? "The transcript could not be read."}</p>;
  }
  return (
    <section aria-label="Transcript" className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-line px-3 py-2">
        <h3 className="text-sm font-semibold">Transcript{transcript.data.language ? ` (${transcript.data.language})` : ""}</h3>
        <div className="flex items-center gap-3 text-xs">
          <label className="flex items-center gap-1 text-slate-300">
            <input type="checkbox" checked={follow} onChange={(event) => setFollow(event.target.checked)} /> Follow video
          </label>
          <input
            type="search"
            aria-label="Search transcript"
            placeholder="Search transcript"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            className="w-36 rounded-md border border-line bg-surface px-2 py-1 text-fg"
          />
        </div>
      </div>
      {query && shown.length === 0 ? <p className="px-3 pt-3 text-sm text-muted">No line mentions “{filter.trim()}”.</p> : null}
      <ol ref={list} aria-label="Transcript lines" className="relative min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 py-2">
        {shown.map(({ cue, index }) => (
          <li key={index} data-cue={index}>
            <button
              type="button"
              aria-current={index === active ? "true" : undefined}
              onClick={() => onSeek(cue.start)}
              className={`flex w-full gap-3 rounded-md px-2 py-1 text-left text-sm ${index === active ? "bg-accent/12 text-fg" : "text-fg-soft hover:bg-white/5"}`}
            >
              <span className="w-14 shrink-0 font-mono text-xs leading-5 text-subtle">{clock(cue.start)}</span>
              <span className="whitespace-pre-line">{cue.text}</span>
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}
