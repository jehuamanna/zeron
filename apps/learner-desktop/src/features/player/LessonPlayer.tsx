import { AlertTriangle, Loader2, RotateCcw } from "lucide-react";
import { useEffect, useReducer, useRef, useState, type SyntheticEvent } from "react";

import { Button } from "../../components/ui/button";
import { describePlayback, initialPlayback, playbackReducer } from "./playbackState";

interface LessonPlayerProps {
  url: string;
  /** Torrent lessons stream verified data and can stall waiting for peers. */
  torrent: boolean;
  /** Share of the lesson verified on this computer, for torrent lessons. */
  verifiedPercent: number | null;
  onRetry: () => void;
}

function formatClock(seconds: number) {
  const whole = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(whole / 60);
  return `${minutes}:${String(whole % 60).padStart(2, "0")}`;
}

/** Seconds buffered past the current position, from the range that contains it. */
function bufferedAhead(video: HTMLVideoElement): number {
  const { buffered, currentTime } = video;
  for (let index = 0; index < buffered.length; index += 1) {
    if (buffered.start(index) <= currentTime + 0.5 && currentTime <= buffered.end(index)) {
      return buffered.end(index) - currentTime;
    }
  }
  return 0;
}

export function LessonPlayer({ url, torrent, verifiedPercent, onRetry }: LessonPlayerProps) {
  const [playback, dispatch] = useReducer(playbackReducer, initialPlayback);
  const [ahead, setAhead] = useState(0);
  const video = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    const timer = window.setInterval(() => dispatch({ type: "tick", at: Date.now() }), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const measure = (event: SyntheticEvent<HTMLVideoElement>) => setAhead(bufferedAhead(event.currentTarget));
  const busy = playback.state === "loading" || playback.state === "buffering";
  const failed = playback.state === "error";
  const stalled = playback.state === "stalled";

  return (
    <div>
      <video
        ref={video}
        className="aspect-video w-full rounded-2xl bg-black"
        src={url}
        controls
        autoPlay
        preload="metadata"
        onLoadStart={() => dispatch({ type: "loadstart", at: Date.now() })}
        onWaiting={() => dispatch({ type: "waiting", at: Date.now() })}
        onSeeking={() => dispatch({ type: "seeking", at: Date.now() })}
        onProgress={(event) => {
          measure(event);
          dispatch({ type: "progress", at: Date.now() });
        }}
        onTimeUpdate={(event) => {
          measure(event);
          dispatch({ type: "timeupdate", position: event.currentTarget.currentTime, paused: event.currentTarget.paused, at: Date.now() });
        }}
        onPlaying={() => dispatch({ type: "playing", at: Date.now() })}
        onPause={() => dispatch({ type: "pause", at: Date.now() })}
        onEnded={() => dispatch({ type: "ended" })}
        onError={(event) => dispatch({ type: "error", code: event.currentTarget.error?.code ?? 4 })}
      >
        This system cannot play the selected video format.
      </video>
      <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm">
        <p
          role={failed ? "alert" : "status"}
          aria-label="Playback status"
          data-state={playback.state}
          className={`flex items-center gap-2 ${failed ? "text-red-300" : stalled ? "text-amber-200" : "text-slate-300"}`}
        >
          {busy ? <Loader2 size={15} className="animate-spin" /> : null}
          {failed || stalled ? <AlertTriangle size={15} /> : null}
          {describePlayback(playback, torrent)}
        </p>
        <p className="text-xs text-slate-400">
          <span title="Data the player has loaded ahead of the current position">Buffered: {formatClock(ahead)} ahead</span>
          {torrent && verifiedPercent !== null ? (
            <span title="Lesson data checked against the torrent and kept on this computer"> · Verified on this computer: {verifiedPercent}%</span>
          ) : null}
        </p>
        {failed ? (
          <Button variant="ghost" onClick={onRetry}>
            <RotateCcw size={16} /> Retry
          </Button>
        ) : null}
      </div>
    </div>
  );
}
