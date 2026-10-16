import { AlertTriangle, Clock, Gauge, Loader2, RotateCcw, RotateCw } from "lucide-react";
import { useEffect, useImperativeHandle, useReducer, useRef, useState, type Ref, type SyntheticEvent } from "react";

import { Button } from "../../components/ui/button";
import { describePlayback, initialPlayback, playbackReducer } from "./playbackState";

/** What other panels may ask of the player. */
export interface PlayerHandle {
  currentTime: () => number;
  seek: (seconds: number) => void;
  /** Moves by `seconds` from the current position, clamped to the video. */
  skip: (seconds: number) => void;
  togglePlay: () => void;
}

const SPEEDS = [0.75, 1, 1.25, 1.5, 1.75, 2];

interface LessonPlayerProps {
  /** Lets notes read the current time and jump to a timestamp. */
  handle?: Ref<PlayerHandle>;
  url: string;
  /** Torrent lessons stream verified data and can stall waiting for peers. */
  torrent: boolean;
  /** Share of the lesson verified on this computer, for torrent lessons. */
  verifiedPercent: number | null;
  onRetry: () => void;
  /** The course's downloads are paused, so missing data will not arrive. */
  transfersPaused?: boolean;
  /** Saved position to resume from, in seconds. */
  startAt?: number | null;
  /** Called while playing (at most every few seconds), on pause, at the end, and when closed. */
  onPosition?: (positionSeconds: number, durationSeconds: number) => void;
  /** Starts a note at the current moment; shown as a toolbar button. */
  onAddNote?: (seconds: number) => void;
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

export function LessonPlayer({ handle, url, torrent, verifiedPercent, onRetry, startAt = null, onPosition, onAddNote, transfersPaused = false }: LessonPlayerProps) {
  const [playback, dispatch] = useReducer(playbackReducer, initialPlayback);
  const [ahead, setAhead] = useState(0);
  const [resumedAt, setResumedAt] = useState<number | null>(null);
  const [speed, setSpeed] = useState(1);
  const [position, setPosition] = useState(0);
  const video = useRef<HTMLVideoElement>(null);
  const lastSaved = useRef(0);
  const latest = useRef<{ position: number; duration: number } | null>(null);
  const report = useRef(onPosition);
  report.current = onPosition;
  useImperativeHandle(handle, () => ({
    currentTime: () => video.current?.currentTime ?? 0,
    seek: (seconds: number) => {
      if (!video.current) return;
      video.current.currentTime = seconds;
      void video.current.play().catch(() => undefined);
    },
    skip: (seconds: number) => skip(seconds),
    togglePlay: () => {
      const element = video.current;
      if (!element) return;
      if (element.paused) void element.play().catch(() => undefined);
      else element.pause();
    },
  }), []);

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

  const skip = (seconds: number) => {
    const element = video.current;
    if (!element) return;
    const end = Number.isFinite(element.duration) ? element.duration : Infinity;
    element.currentTime = Math.min(end, Math.max(0, element.currentTime + seconds));
  };
  const toolButton = "inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted hover:bg-white/10 hover:text-fg";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl bg-video">
      <video
        ref={video}
        className="h-full w-full bg-video object-contain"
        src={url}
        controls
        autoPlay
        preload="metadata"
        onLoadStart={() => dispatch({ type: "loadstart", at: Date.now() })}
        onLoadedMetadata={(event) => {
          event.currentTarget.playbackRate = speed;
          resume(event.currentTarget);
        }}
        onWaiting={() => dispatch({ type: "waiting", at: Date.now() })}
        onSeeking={() => dispatch({ type: "seeking", at: Date.now() })}
        onProgress={(event) => {
          measure(event);
          dispatch({ type: "progress", at: Date.now() });
        }}
        onTimeUpdate={(event) => {
          measure(event);
          setPosition(event.currentTarget.currentTime);
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
        <p className="absolute left-3 top-3 flex flex-wrap items-center gap-3 rounded-lg bg-[#000]/70 px-3 py-1.5 text-xs text-[#f5f5f4] backdrop-blur">
          Resumed where you left off ({formatClock(resumedAt)}).
          <button
            type="button"
            className="font-medium text-[#fda4af] underline-offset-2 hover:underline"
            onClick={() => {
              if (video.current) video.current.currentTime = 0;
              setResumedAt(null);
            }}
          >
            Start over
          </button>
          <button type="button" aria-label="Dismiss" className="text-[#a8a29e] hover:text-[#f5f5f4]" onClick={() => setResumedAt(null)}>✕</button>
        </p>
      ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 pt-2 text-sm">
        <div className="flex items-center gap-0.5">
          <button type="button" className={toolButton} aria-label="Back 10 seconds" title="Back 10 seconds (J)" onClick={() => skip(-10)}>
            <RotateCcw size={14} /> 10
          </button>
          <button type="button" className={toolButton} aria-label="Forward 10 seconds" title="Forward 10 seconds (L)" onClick={() => skip(10)}>
            <RotateCw size={14} /> 10
          </button>
          <label className={`${toolButton} cursor-pointer`} title="Playback speed">
            <Gauge size={14} />
            <span className="sr-only">Playback speed</span>
            <select
              value={speed}
              onChange={(event) => {
                const next = Number(event.target.value);
                setSpeed(next);
                if (video.current) video.current.playbackRate = next;
              }}
              className="cursor-pointer bg-transparent text-xs text-fg outline-none"
            >
              {SPEEDS.map((value) => <option key={value} value={value}>{value}×</option>)}
            </select>
          </label>
          {onAddNote ? (
            <button
              type="button"
              className={`${toolButton} text-accent-text`}
              title="Add a note at this moment (N)"
              onClick={() => onAddNote(Math.floor(video.current?.currentTime ?? position))}
            >
              <Clock size={14} /> Note at {formatClock(position)}
            </button>
          ) : null}
        </div>
        <p
          role={failed ? "alert" : "status"}
          aria-label="Playback status"
          data-state={playback.state}
          className={`flex items-center gap-1.5 text-xs ${failed ? "text-red-300" : stalled ? "text-amber-200" : "text-muted"}`}
        >
          {busy ? <Loader2 size={13} className="animate-spin" /> : null}
          {failed || stalled ? <AlertTriangle size={13} /> : null}
          {describePlayback(playback, torrent, transfersPaused)}
        </p>
        <p className="ml-auto text-xs text-subtle">
          <span title="Data the player has loaded ahead of the current position">Buffered: {formatClock(ahead)} ahead</span>
          {torrent && verifiedPercent !== null ? (
            <span title="Lesson data checked against the torrent and kept on this computer"> · Verified on this computer: {verifiedPercent}%</span>
          ) : null}
        </p>
        {failed ? (
          <Button variant="ghost" size="sm" onClick={onRetry}>
            <RotateCcw size={14} /> Retry
          </Button>
        ) : null}
      </div>
    </div>
  );
}
