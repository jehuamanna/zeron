import { AlertTriangle, Clock, Loader2, MonitorPlay, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref } from "react";

import { Button } from "../../components/ui/button";
import { usePlayerPrefs } from "../../stores/player";
import { formatClock, type PlayerHandle, type TimelineMarker } from "./LessonPlayer";

/** Resume only from a meaningful point. */
const RESUME_AFTER_SECONDS = 5;
const SAVE_EVERY_MS = 5000;
/** YouTube's player states. */
const ENDED = 0;
const PLAYING = 1;
const PAUSED = 2;

type Status = "loading" | "playing" | "paused" | "ended" | "error";

/** What the player page in the gateway frame reports (ADR 0018). */
type PlayerMessage =
  | { zeron: "youtube"; type: "ready"; duration: number }
  | { zeron: "youtube"; type: "time"; position: number; duration: number; state: number }
  | { zeron: "youtube"; type: "ended" }
  | { zeron: "youtube"; type: "error"; code: number | string };

type PlayerCommand =
  | { type: "seek"; seconds: number }
  | { type: "play" | "pause" | "toggle" | "mute" }
  | { type: "volume"; step: number };

/** Why YouTube would not play, in the learner's words. */
export function describeYouTubeError(code: number | string): string {
  if (code === "offline") return "YouTube could not be reached. Check the internet connection, then retry.";
  if (code === 101 || code === 150) return "The owner of this video does not allow it to be played outside YouTube.";
  if (code === 100) return "This video is unavailable. It may be private or removed.";
  if (code === 152 || code === 153) return "YouTube refused to play the video in Zeron.";
  return "YouTube could not play this video.";
}

function isPlayerMessage(data: unknown): data is PlayerMessage {
  return typeof data === "object" && data !== null && (data as { zeron?: unknown }).zeron === "youtube" && typeof (data as { type?: unknown }).type === "string";
}

interface YouTubePlayerProps {
  handle?: Ref<PlayerHandle>;
  /** The gateway page's capability URL, with the video in its fragment. */
  url: string;
  onRetry: () => void;
  startAt?: number | null;
  onPosition?: (positionSeconds: number, durationSeconds: number) => void;
  onAddNote?: (seconds: number) => void;
  markers?: TimelineMarker[];
  upNext?: { title: string } | null;
  onNext?: () => void;
  previous?: { title: string } | null;
  onPrevious?: () => void;
}

/**
 * A YouTube lesson: YouTube's own player, in a page the media gateway serves
 * outside the app's origin, driven by messages. Notes, markers, and saved
 * positions work as for any lesson.
 */
export function YouTubePlayer({ handle, url, onRetry, startAt = null, onPosition, onAddNote, markers = [], upNext = null, onNext, previous = null, onPrevious }: YouTubePlayerProps) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<number | string | null>(null);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const latest = useRef({ position: 0, duration: 0 });
  const lastSaved = useRef(0);
  const report = useRef(onPosition);
  report.current = onPosition;

  const resumeAt = startAt !== null && startAt >= RESUME_AFTER_SECONDS ? Math.floor(startAt) : 0;
  const source = resumeAt ? `${url}&t=${resumeAt}` : url;
  const origin = useMemo(() => new URL(url).origin, [url]);

  const send = (command: PlayerCommand) => frame.current?.contentWindow?.postMessage({ zeron: "youtube", ...command }, origin);

  useImperativeHandle(handle, () => ({
    currentTime: () => latest.current.position,
    seek: (seconds: number) => {
      send({ type: "seek", seconds });
      send({ type: "play" });
      latest.current.position = seconds;
      setPosition(seconds);
    },
    skip: (seconds: number) => {
      const end = latest.current.duration || Infinity;
      const target = Math.min(end, Math.max(0, latest.current.position + seconds));
      send({ type: "seek", seconds: target });
      latest.current.position = target;
      setPosition(target);
    },
    togglePlay: () => send({ type: "toggle" }),
    toggleMute: () => send({ type: "mute" }),
    nudgeVolume: (step: number) => send({ type: "volume", step }),
    // YouTube shows its own captions and full-screen controls.
    toggleCaptions: () => undefined,
    toggleFullscreen: () => void frame.current?.requestFullscreen?.().catch(() => undefined),
    // `send` only reads refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), [origin]);

  useEffect(() => {
    const remember = (force: boolean) => {
      const { position: at, duration: length } = latest.current;
      if (length <= 0) return;
      const now = Date.now();
      if (force || now - lastSaved.current >= SAVE_EVERY_MS) {
        lastSaved.current = now;
        report.current?.(at, length);
      }
    };
    const listener = (event: MessageEvent) => {
      // Only the frame this player made, served by the gateway, may talk to it.
      if (event.source !== frame.current?.contentWindow || event.origin !== origin || !isPlayerMessage(event.data)) return;
      const message = event.data;
      if (message.type === "error") {
        setError(message.code);
        setStatus("error");
        return;
      }
      if (message.type === "ended") {
        setStatus("ended");
        remember(true);
        return;
      }
      const length = Number.isFinite(message.duration) && message.duration > 0 ? message.duration : latest.current.duration;
      latest.current.duration = length;
      setDuration(length);
      if (message.type === "ready") return;
      if (Number.isFinite(message.position)) {
        latest.current.position = message.position;
        setPosition(message.position);
      }
      if (message.state === PLAYING) {
        setStatus("playing");
        setError(null);
        remember(false);
      } else if (message.state === PAUSED) {
        setStatus("paused");
        remember(true);
      } else if (message.state === ENDED) {
        setStatus("ended");
        remember(true);
      }
    };
    window.addEventListener("message", listener);
    return () => {
      window.removeEventListener("message", listener);
      if (latest.current.duration > 0) report.current?.(latest.current.position, latest.current.duration);
    };
  }, [origin]);

  // The learner's mute choice carries over; YouTube keeps its own volume.
  useEffect(() => {
    if (status === "playing" && usePlayerPrefs.getState().muted) send({ type: "mute" });
    // Only once, when playback first starts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status === "playing"]);

  const known = duration > 0;
  const percent = (seconds: number) => `${Math.min(100, Math.max(0, (seconds / duration) * 100))}%`;
  const control = "grid size-8 shrink-0 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl bg-video">
        <iframe
          ref={frame}
          key={source}
          src={source}
          title="YouTube video player"
          className="h-full w-full border-0"
          allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
          allowFullScreen
          referrerPolicy="no-referrer"
        />
        {status === "loading" ? (
          <div aria-hidden className="pointer-events-none absolute inset-0 grid place-items-center">
            <Loader2 size={40} className="animate-spin text-[#f5f5f4]/80" />
          </div>
        ) : null}
      </div>

      {known && markers.length ? (
        <div aria-label="Notes on this video" role="group" className="relative mt-2 h-3 rounded-full bg-white/10">
          <div aria-hidden className="absolute inset-y-0 left-0 rounded-full bg-white/20" style={{ width: percent(position) }} />
          {markers.map((marker, index) => (
            <button
              key={index}
              type="button"
              aria-label={`Go to the note at ${formatClock(marker.seconds)}: ${marker.label.slice(0, 48)}`}
              title={`${formatClock(marker.seconds)} · ${marker.label.slice(0, 48)}`}
              onClick={() => {
                send({ type: "seek", seconds: marker.seconds });
                send({ type: "play" });
              }}
              className="absolute top-1/2 h-3 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#fde68a] shadow hover:scale-125"
              style={{ left: percent(marker.seconds) }}
            />
          ))}
        </div>
      ) : null}

      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 pt-2 text-sm">
        <p role={status === "error" ? "alert" : "status"} aria-label="Playback status" data-state={status} className={`flex items-center gap-1.5 text-xs ${status === "error" ? "text-red-300" : "text-muted"}`}>
          {status === "loading" ? <Loader2 size={13} className="animate-spin" /> : null}
          {status === "error" ? <AlertTriangle size={13} /> : <MonitorPlay size={13} />}
          {status === "error" && error !== null
            ? describeYouTubeError(error)
            : status === "loading"
              ? "Loading YouTube…"
              : status === "ended"
                ? "Finished"
                : status === "paused"
                  ? "Paused · YouTube"
                  : "Playing from YouTube"}
        </p>
        {known ? <span className="font-mono text-xs tabular-nums text-subtle">{formatClock(position)} / {formatClock(duration)}</span> : null}
        <div className="ml-auto flex items-center gap-1">
          {status === "error" ? (
            <Button variant="ghost" size="sm" onClick={onRetry}>Retry</Button>
          ) : null}
          {onAddNote ? (
            <Button variant="ghost" size="sm" title="Add a note at this moment (N)" onClick={() => onAddNote(Math.floor(latest.current.position))}>
              <Clock size={14} /> Note at {formatClock(position)}
            </Button>
          ) : null}
          {previous ? (
            <button type="button" className={control} aria-label={`Previous lesson: ${previous.title}`} onClick={() => onPrevious?.()}>
              <SkipBack size={16} />
            </button>
          ) : null}
          {upNext ? (
            <button type="button" className={control} aria-label={`Next lesson: ${upNext.title}`} onClick={() => onNext?.()}>
              <SkipForward size={16} />
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
