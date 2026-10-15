import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";

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
export function TranscriptPanel({ courseId, mediaResourceId, currentTime, onSeek }: {
  courseId: string;
  mediaResourceId: string;
  currentTime: () => number;
  onSeek: (seconds: number) => void;
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

  if (transcript.isPending || transcript.data === null) return null;
  if (transcript.isError) {
    return <p role="alert" className="mt-4 text-sm text-red-300">{asCommandError(transcript.error)?.message ?? "The transcript could not be read."}</p>;
  }
  return (
    <section aria-label="Transcript" className="mt-6 rounded-2xl border border-white/10 bg-black/20 p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <h3 className="font-semibold">Transcript{transcript.data.language ? ` (${transcript.data.language})` : ""}</h3>
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
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-slate-100"
          />
        </div>
      </div>
      {query && shown.length === 0 ? <p className="text-sm text-slate-400">No line mentions “{filter.trim()}”.</p> : null}
      <ol ref={list} aria-label="Transcript lines" className="max-h-72 space-y-1 overflow-y-auto pr-2">
        {shown.map(({ cue, index }) => (
          <li key={index} data-cue={index}>
            <button
              type="button"
              aria-current={index === active ? "true" : undefined}
              onClick={() => onSeek(cue.start)}
              className={`flex w-full gap-3 rounded-md px-2 py-1 text-left text-sm ${index === active ? "bg-cyan-300/15 text-cyan-50" : "text-slate-300 hover:bg-white/5"}`}
            >
              <span className="w-14 shrink-0 font-mono text-xs leading-5 text-slate-500">{clock(cue.start)}</span>
              <span className="whitespace-pre-line">{cue.text}</span>
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}
