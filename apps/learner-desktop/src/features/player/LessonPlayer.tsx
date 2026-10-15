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
  /** Saved position to resume from, in seconds. */
  startAt?: number | null;
  /** Called while playing (at most every few seconds), on pause, at the end, and when closed. */
  onPosition?: (positionSeconds: number, durationSeconds: number) => void;
}

/** Resume only from a meaningful point that is not right at the end. */
const RESUME_AFTER_SECONDS = 5;
const RESUME_BEFORE_END_SECONDS = 10;
const SAVE_EVERY_MS = 5000;

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

export function LessonPlayer({ url, torrent, verifiedPercent, onRetry, startAt = null, onPosition }: LessonPlayerProps) {
  const [playback, dispatch] = useReducer(playbackReducer, initialPlayback);
  const [ahead, setAhead] = useState(0);
  const [resumedAt, setResumedAt] = useState<number | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const lastSaved = useRef(0);
  const latest = useRef<{ position: number; duration: number } | null>(null);
  const report = useRef(onPosition);
  report.current = onPosition;

  useEffect(() => {
    const timer = window.setInterval(() => dispatch({ type: "tick", at: Date.now() }), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(
    () => () => {
      if (latest.current) report.current?.(latest.current.position, latest.current.duration);
    },
    [],
  );

  const remember = (element: HTMLVideoElement, force: boolean) => {
    const duration = element.duration;
    if (!Number.isFinite(duration) || duration <= 0 || element.seeking) return;
    latest.current = { position: element.currentTime, duration };
    const now = Date.now();
    if (force || now - lastSaved.current >= SAVE_EVERY_MS) {
      lastSaved.current = now;
      report.current?.(element.currentTime, duration);
    }
  };

  const resume = (element: HTMLVideoElement) => {
    if (startAt !== null && startAt >= RESUME_AFTER_SECONDS && startAt < element.duration - RESUME_BEFORE_END_SECONDS) {
      element.currentTime = startAt;
      setResumedAt(startAt);
    }
  };

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
        onLoadedMetadata={(event) => resume(event.currentTarget)}
        onWaiting={() => dispatch({ type: "waiting", at: Date.now() })}
        onSeeking={() => dispatch({ type: "seeking", at: Date.now() })}
        onProgress={(event) => {
          measure(event);
          dispatch({ type: "progress", at: Date.now() });
        }}
        onTimeUpdate={(event) => {
          measure(event);
          dispatch({ type: "timeupdate", position: event.currentTarget.currentTime, paused: event.currentTarget.paused, at: Date.now() });
          if (!event.currentTarget.paused) remember(event.currentTarget, false);
        }}
        onPlaying={() => dispatch({ type: "playing", at: Date.now() })}
        onPause={(event) => {
          dispatch({ type: "pause", at: Date.now() });
          remember(event.currentTarget, true);
        }}
        onEnded={(event) => {
          dispatch({ type: "ended" });
          remember(event.currentTarget, true);
        }}
        onError={(event) => dispatch({ type: "error", code: event.currentTarget.error?.code ?? 4 })}
      >
        This system cannot play the selected video format.
      </video>
      {resumedAt !== null ? (
        <p className="mt-3 flex flex-wrap items-center gap-3 text-xs text-slate-400">
          Resumed where you left off ({formatClock(resumedAt)}).
          <button
            type="button"
            className="text-cyan-200 underline-offset-2 hover:underline"
            onClick={() => {
              if (video.current) video.current.currentTime = 0;
              setResumedAt(null);
            }}
          >
            Start over
          </button>
        </p>
      ) : null}
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
