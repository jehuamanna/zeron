import {
  AlertTriangle,
  Captions,
  Clock,
  Loader2,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  RotateCcw,
  RotateCw,
  Settings,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useCallback, useEffect, useImperativeHandle, useReducer, useRef, useState, type Ref, type SyntheticEvent } from "react";

import { Button } from "../../components/ui/button";
import type { TranscriptCue } from "../../services/library";
import { usePlayerPrefs } from "../../stores/player";
import { activeCueIndex } from "../transcripts/TranscriptPanel";
import { describePlayback, initialPlayback, playbackReducer } from "./playbackState";

/** What other panels and shortcuts may ask of the player. */
export interface PlayerHandle {
  currentTime: () => number;
  seek: (seconds: number) => void;
  /** Moves by `seconds` from the current position, clamped to the video. */
  skip: (seconds: number) => void;
  togglePlay: () => void;
  toggleMute: () => void;
  /** Changes the volume by `step` (from -1 to 1). */
  nudgeVolume: (step: number) => void;
  toggleCaptions: () => void;
  toggleFullscreen: () => void;
}

/** A note on the timeline. */
export interface TimelineMarker {
  seconds: number;
  label: string;
}

export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
/** Seconds the "Up next" countdown waits before the next lesson starts. */
export const AUTOPLAY_SECONDS = 5;
/** Controls hide after this long without the pointer moving while playing. */
const IDLE_MS = 2500;

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
  /** Starts a note at the current moment. */
  onAddNote?: (seconds: number) => void;
  /** The lesson's notes, shown on the timeline. */
  markers?: TimelineMarker[];
  /** Transcript lines, shown as captions when turned on. */
  cues?: TranscriptCue[] | null;
  /** The next lesson, for "Up next" and autoplay. */
  upNext?: { title: string } | null;
  onNext?: () => void;
}

/** Resume only from a meaningful point that is not right at the end. */
const RESUME_AFTER_SECONDS = 5;
const RESUME_BEFORE_END_SECONDS = 10;
const SAVE_EVERY_MS = 5000;

export function formatClock(seconds: number) {
  const whole = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, "0");
  return hours ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
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

function bufferedRanges(video: HTMLVideoElement): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  for (let index = 0; index < video.buffered.length; index += 1) ranges.push([video.buffered.start(index), video.buffered.end(index)]);
  return ranges;
}

export function LessonPlayer({ handle, url, torrent, verifiedPercent, onRetry, startAt = null, onPosition, onAddNote, transfersPaused = false, markers = [], cues = null, upNext = null, onNext }: LessonPlayerProps) {
  const [playback, dispatch] = useReducer(playbackReducer, initialPlayback);
  const [ahead, setAhead] = useState(0);
  const [ranges, setRanges] = useState<Array<[number, number]>>([]);
  const [resumedAt, setResumedAt] = useState<number | null>(null);
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(0);
  const [paused, setPaused] = useState(true);
  const [ended, setEnded] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [menu, setMenu] = useState<"speed" | "settings" | null>(null);
  const [remaining, setRemaining] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [awake, setAwake] = useState(true);
  const prefs = usePlayerPrefs();
  const video = useRef<HTMLVideoElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  const idle = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSaved = useRef(0);
  const latest = useRef<{ position: number; duration: number } | null>(null);
  const report = useRef(onPosition);
  report.current = onPosition;

  const play = () => {
    // Some engines return nothing from play(); a refused autoplay is not an error here.
    if (video.current) void Promise.resolve(video.current.play()).catch(() => undefined);
  };
  const togglePlay = () => {
    const element = video.current;
    if (!element) return;
    if (element.paused) play();
    else element.pause();
  };
  const skip = (seconds: number) => {
    const element = video.current;
    if (!element) return;
    const end = Number.isFinite(element.duration) ? element.duration : Infinity;
    element.currentTime = Math.min(end, Math.max(0, element.currentTime + seconds));
    setPosition(element.currentTime);
  };
  const seekTo = (seconds: number) => {
    const element = video.current;
    if (!element) return;
    element.currentTime = seconds;
    setPosition(seconds);
  };
  const toggleFullscreen = () => {
    if (document.fullscreenElement) void document.exitFullscreen?.().catch(() => undefined);
    else void frame.current?.requestFullscreen?.().catch(() => undefined);
  };

  useImperativeHandle(handle, () => ({
    currentTime: () => video.current?.currentTime ?? 0,
    seek: (seconds: number) => {
      seekTo(seconds);
      play();
    },
    skip,
    togglePlay,
    toggleMute: () => usePlayerPrefs.getState().set({ muted: !usePlayerPrefs.getState().muted }),
    nudgeVolume: (step: number) => {
      const { volume } = usePlayerPrefs.getState();
      usePlayerPrefs.getState().set({ volume: Math.min(1, Math.max(0, Math.round((volume + step) * 100) / 100)), muted: false });
    },
    toggleCaptions: () => usePlayerPrefs.getState().set({ captions: !usePlayerPrefs.getState().captions }),
    toggleFullscreen,
    // The helpers only read refs and stores.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), []);

  // Apply the learner's preferences to the element.
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    element.playbackRate = prefs.speed;
    element.volume = prefs.volume;
    element.muted = prefs.muted;
  }, [prefs.speed, prefs.volume, prefs.muted]);

  useEffect(() => {
    const timer = window.setInterval(() => dispatch({ type: "tick", at: Date.now() }), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(
    () => () => {
      if (latest.current) report.current?.(latest.current.position, latest.current.duration);
      if (idle.current) clearTimeout(idle.current);
    },
    [],
  );

  useEffect(() => {
    const follow = () => setFullscreen(document.fullscreenElement === frame.current);
    document.addEventListener("fullscreenchange", follow);
    return () => document.removeEventListener("fullscreenchange", follow);
  }, []);

  // "Up next" counts down, then starts the next lesson.
  useEffect(() => {
    if (countdown === null) return;
    if (countdown <= 0) {
      setCountdown(null);
      onNext?.();
      return;
    }
    const timer = setTimeout(() => setCountdown(countdown - 1), 1000);
    return () => clearTimeout(timer);
  }, [countdown, onNext]);

  // Controls stay while paused, and hide after a moment of stillness while playing.
  const wake = useCallback(() => {
    setAwake(true);
    if (idle.current) clearTimeout(idle.current);
    idle.current = setTimeout(() => setAwake(false), IDLE_MS);
  }, []);

  const remember = (element: HTMLVideoElement, force: boolean) => {
    const length = element.duration;
    if (!Number.isFinite(length) || length <= 0 || element.seeking) return;
    latest.current = { position: element.currentTime, duration: length };
    const now = Date.now();
    if (force || now - lastSaved.current >= SAVE_EVERY_MS) {
      lastSaved.current = now;
      report.current?.(element.currentTime, length);
    }
  };

  const resume = (element: HTMLVideoElement) => {
    if (startAt !== null && startAt >= RESUME_AFTER_SECONDS && startAt < element.duration - RESUME_BEFORE_END_SECONDS) {
      element.currentTime = startAt;
      setResumedAt(startAt);
    }
  };

  const measure = (event: SyntheticEvent<HTMLVideoElement>) => {
    setAhead(bufferedAhead(event.currentTarget));
    setRanges(bufferedRanges(event.currentTarget));
  };
  const busy = playback.state === "loading" || playback.state === "buffering";
  const failed = playback.state === "error";
  const stalled = playback.state === "stalled";
  const showControls = paused || awake || menu !== null || ended;
  const cue = prefs.captions && cues?.length ? cues[activeCueIndex(cues, position)] : undefined;
  const volumeIcon = prefs.muted || prefs.volume === 0 ? <VolumeX size={17} /> : prefs.volume < 0.5 ? <Volume1 size={17} /> : <Volume2 size={17} />;
  const control = "grid size-8 shrink-0 place-items-center rounded-md text-[#f5f5f4] hover:bg-[#fff]/15 disabled:opacity-40";
  const pip = typeof document !== "undefined" && "pictureInPictureEnabled" in document && document.pictureInPictureEnabled;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        ref={frame}
        className={`relative min-h-0 flex-1 overflow-hidden bg-video ${fullscreen ? "" : "rounded-xl"} ${showControls ? "" : "cursor-none"}`}
        onPointerMove={wake}
        onPointerLeave={() => setAwake(false)}
      >
        <video
          ref={video}
          className="h-full w-full bg-video object-contain"
          src={url}
          autoPlay
          preload="metadata"
          onClick={() => {
            setMenu(null);
            togglePlay();
          }}
          onDoubleClick={toggleFullscreen}
          onLoadStart={() => dispatch({ type: "loadstart", at: Date.now() })}
          onLoadedMetadata={(event) => {
            const element = event.currentTarget;
            element.playbackRate = usePlayerPrefs.getState().speed;
            element.volume = usePlayerPrefs.getState().volume;
            element.muted = usePlayerPrefs.getState().muted;
            setDuration(Number.isFinite(element.duration) ? element.duration : 0);
            resume(element);
          }}
          onDurationChange={(event) => setDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)}
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
          onPlay={() => {
            setPaused(false);
            setEnded(false);
            setCountdown(null);
            wake();
          }}
          onPlaying={() => dispatch({ type: "playing", at: Date.now() })}
          onPause={(event) => {
            setPaused(true);
            dispatch({ type: "pause", at: Date.now() });
            remember(event.currentTarget, true);
          }}
          onEnded={(event) => {
            setPaused(true);
            setEnded(true);
            dispatch({ type: "ended" });
            remember(event.currentTarget, true);
            if (upNext && usePlayerPrefs.getState().autoplayNext) setCountdown(AUTOPLAY_SECONDS);
          }}
          onError={(event) => dispatch({ type: "error", code: event.currentTarget.error?.code ?? 4 })}
        >
          This system cannot play the selected video format.
        </video>

        {busy && !paused ? (
          <div aria-hidden className="pointer-events-none absolute inset-0 grid place-items-center">
            <Loader2 size={40} className="animate-spin text-[#f5f5f4]/80" />
          </div>
        ) : null}
        {paused && !ended && !failed && !busy ? (
          <button type="button" aria-label="Play video" onClick={play} className="absolute left-1/2 top-1/2 grid size-16 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full bg-[#000]/55 text-[#fff] backdrop-blur transition hover:scale-105 hover:bg-[#000]/70">
            <Play size={28} className="translate-x-0.5" fill="currentColor" />
          </button>
        ) : null}

        {resumedAt !== null ? (
          <p className="absolute left-3 top-3 flex flex-wrap items-center gap-3 rounded-lg bg-[#000]/70 px-3 py-1.5 text-xs text-[#f5f5f4] backdrop-blur">
            Resumed where you left off ({formatClock(resumedAt)}).
            <button
              type="button"
              className="font-medium text-[#fda4af] underline-offset-2 hover:underline"
              onClick={() => {
                seekTo(0);
                setResumedAt(null);
              }}
            >
              Start over
            </button>
            <button type="button" aria-label="Dismiss" className="text-[#a8a29e] hover:text-[#f5f5f4]" onClick={() => setResumedAt(null)}>✕</button>
          </p>
        ) : null}

        {ended && upNext ? (
          <div role="dialog" aria-label="Up next" className="absolute inset-0 grid place-items-center bg-[#000]/70 p-4 text-center text-[#f5f5f4] backdrop-blur-sm">
            <div>
              <p className="text-xs uppercase tracking-[0.2em] text-[#a8a29e]">Up next</p>
              <p className="mt-1 font-display text-2xl font-semibold">{upNext.title}</p>
              {countdown !== null ? <p aria-live="polite" className="mt-2 text-sm text-[#d6d3d1]">Starting in {countdown}…</p> : null}
              <div className="mt-4 flex items-center justify-center gap-2">
                <button type="button" onClick={() => onNext?.()} className="inline-flex items-center gap-1.5 rounded-lg bg-[#fff] px-4 py-2 text-sm font-semibold text-[#1c1917] hover:bg-[#e7e5e4]">
                  <SkipForward size={15} /> Play now
                </button>
                {countdown !== null ? (
                  <button type="button" onClick={() => setCountdown(null)} className="rounded-lg px-4 py-2 text-sm text-[#d6d3d1] hover:bg-[#fff]/10">Cancel</button>
                ) : (
                  <button type="button" onClick={() => { seekTo(0); play(); }} className="rounded-lg px-4 py-2 text-sm text-[#d6d3d1] hover:bg-[#fff]/10">Watch again</button>
                )}
              </div>
            </div>
          </div>
        ) : null}

        {cue ? (
          <p className={`pointer-events-none absolute inset-x-0 mx-auto w-fit max-w-[85%] whitespace-pre-line rounded-md bg-[#000]/75 px-3 py-1 text-center text-[15px] leading-snug text-[#fff] transition-all sm:text-lg ${showControls ? "bottom-24" : "bottom-6"}`}>
            {cue.text}
          </p>
        ) : null}

        {/* Control bar */}
        <div
          className={`absolute inset-x-0 bottom-0 bg-gradient-to-t from-[#000]/85 via-[#000]/50 to-transparent px-3 pb-2 pt-8 transition-opacity ${showControls ? "opacity-100" : "pointer-events-none opacity-0"}`}
          onFocus={wake}
        >
          <Timeline
            position={position}
            duration={duration}
            ranges={ranges}
            verifiedPercent={torrent ? verifiedPercent : null}
            markers={markers}
            onSeek={seekTo}
          />
          <div className="mt-1 flex items-center gap-0.5">
            <button type="button" className={control} aria-label={paused ? "Play" : "Pause"} title={paused ? "Play (K)" : "Pause (K)"} onClick={togglePlay}>
              {paused ? <Play size={18} fill="currentColor" /> : <Pause size={18} fill="currentColor" />}
            </button>
            <button type="button" className={control} aria-label="Back 10 seconds" title="Back 10 seconds (J)" onClick={() => skip(-10)}><RotateCcw size={17} /></button>
            <button type="button" className={control} aria-label="Forward 10 seconds" title="Forward 10 seconds (L)" onClick={() => skip(10)}><RotateCw size={17} /></button>
            <div className="group/volume flex items-center">
              <button type="button" className={control} aria-label={prefs.muted ? "Unmute" : "Mute"} title={prefs.muted ? "Unmute (M)" : "Mute (M)"} onClick={() => prefs.set({ muted: !prefs.muted })}>
                {volumeIcon}
              </button>
              <input
                type="range"
                aria-label="Volume"
                min={0}
                max={1}
                step={0.05}
                value={prefs.muted ? 0 : prefs.volume}
                onChange={(event) => prefs.set({ volume: Number(event.target.value), muted: Number(event.target.value) === 0 })}
                className="w-0 cursor-pointer accent-[#fff] opacity-0 transition-all group-focus-within/volume:w-20 group-focus-within/volume:opacity-100 group-hover/volume:w-20 group-hover/volume:opacity-100"
              />
            </div>
            <button
              type="button"
              aria-label={remaining ? "Show elapsed time" : "Show time left"}
              onClick={() => setRemaining(!remaining)}
              className="ml-1 rounded px-1 font-mono text-xs tabular-nums text-[#e7e5e4] hover:bg-[#fff]/10"
            >
              {remaining ? `-${formatClock(duration - position)}` : formatClock(position)} / {formatClock(duration)}
            </button>

            <div className="ml-auto flex items-center gap-0.5">
              {onAddNote ? (
                <button
                  type="button"
                  className="inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-[#f5f5f4] hover:bg-[#fff]/15"
                  title="Add a note at this moment (N)"
                  onClick={() => onAddNote(Math.floor(video.current?.currentTime ?? position))}
                >
                  <Clock size={15} /> <span className="hidden sm:inline">Note at {formatClock(position)}</span>
                </button>
              ) : null}
              {cues?.length ? (
                <button
                  type="button"
                  className={`${control} ${prefs.captions ? "bg-[#fff]/20" : ""}`}
                  aria-label={prefs.captions ? "Hide captions" : "Show captions"}
                  aria-pressed={prefs.captions}
                  title="Captions from the transcript (C)"
                  onClick={() => prefs.set({ captions: !prefs.captions })}
                >
                  <Captions size={17} />
                </button>
              ) : null}
              <div className="relative">
                <button type="button" className="h-8 rounded-md px-2 text-xs font-semibold tabular-nums text-[#f5f5f4] hover:bg-[#fff]/15" aria-label={`Playback speed ${prefs.speed}×`} aria-expanded={menu === "speed"} title="Playback speed" onClick={() => setMenu(menu === "speed" ? null : "speed")}>
                  {prefs.speed}×
                </button>
                {menu === "speed" ? (
                  <div role="menu" aria-label="Playback speed" className="absolute bottom-10 right-0 z-10 w-32 rounded-lg border border-[#fff]/10 bg-[#1c1917]/95 p-1 text-sm text-[#f5f5f4] shadow-xl">
                    {SPEEDS.map((value) => (
                      <button
                        key={value}
                        type="button"
                        role="menuitemradio"
                        aria-checked={prefs.speed === value}
                        onClick={() => {
                          prefs.set({ speed: value });
                          setMenu(null);
                        }}
                        className={`flex w-full items-center justify-between rounded-md px-2 py-1 text-left hover:bg-[#fff]/10 ${prefs.speed === value ? "font-semibold" : ""}`}
                      >
                        {value === 1 ? "Normal" : `${value}×`}
                        {prefs.speed === value ? <span aria-hidden>✓</span> : null}
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
              <div className="relative">
                <button type="button" className={control} aria-label="Player settings" aria-expanded={menu === "settings"} onClick={() => setMenu(menu === "settings" ? null : "settings")}>
                  <Settings size={17} />
                </button>
                {menu === "settings" ? (
                  <div role="menu" aria-label="Player settings" className="absolute bottom-10 right-0 z-10 w-60 rounded-lg border border-[#fff]/10 bg-[#1c1917]/95 p-1 text-sm text-[#f5f5f4] shadow-xl">
                    <button
                      type="button"
                      role="menuitemcheckbox"
                      aria-checked={prefs.autoplayNext}
                      onClick={() => prefs.set({ autoplayNext: !prefs.autoplayNext })}
                      className="flex w-full items-center justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-[#fff]/10"
                    >
                      Autoplay next lesson
                      <Switch on={prefs.autoplayNext} />
                    </button>
                    {cues?.length ? (
                      <button
                        type="button"
                        role="menuitemcheckbox"
                        aria-checked={prefs.captions}
                        onClick={() => prefs.set({ captions: !prefs.captions })}
                        className="flex w-full items-center justify-between gap-3 rounded-md px-2 py-1.5 text-left hover:bg-[#fff]/10"
                      >
                        Captions
                        <Switch on={prefs.captions} />
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
              {pip ? (
                <button
                  type="button"
                  className={control}
                  aria-label="Picture in picture"
                  onClick={() => {
                    const element = video.current as (HTMLVideoElement & { requestPictureInPicture?: () => Promise<unknown> }) | null;
                    if (document.pictureInPictureElement) void document.exitPictureInPicture().catch(() => undefined);
                    else void element?.requestPictureInPicture?.().catch(() => undefined);
                  }}
                >
                  <PictureInPicture2 size={17} />
                </button>
              ) : null}
              {upNext ? (
                <button type="button" className={control} aria-label={`Next lesson: ${upNext.title}`} onClick={() => onNext?.()}>
                  <SkipForward size={17} />
                </button>
              ) : null}
              <button type="button" className={control} aria-label={fullscreen ? "Exit full screen" : "Full screen"} title={fullscreen ? "Exit full screen (F)" : "Full screen (F)"} onClick={toggleFullscreen}>
                {fullscreen ? <Minimize size={17} /> : <Maximize size={17} />}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 pt-2 text-sm">
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

function Switch({ on }: { on: boolean }) {
  return (
    <span aria-hidden className={`relative h-4 w-7 shrink-0 rounded-full transition ${on ? "bg-[#fb7185]" : "bg-[#fff]/25"}`}>
      <span className={`absolute top-0.5 size-3 rounded-full bg-[#fff] transition-all ${on ? "left-3.5" : "left-0.5"}`} />
    </span>
  );
}

/** The seek bar: buffered and verified data, notes as markers, and a time preview under the pointer. */
function Timeline({ position, duration, ranges, verifiedPercent, markers, onSeek }: {
  position: number;
  duration: number;
  ranges: Array<[number, number]>;
  verifiedPercent: number | null;
  markers: TimelineMarker[];
  onSeek: (seconds: number) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [dragging, setDragging] = useState(false);
  const known = duration > 0;
  const percent = (seconds: number) => (known ? `${Math.min(100, Math.max(0, (seconds / duration) * 100))}%` : "0%");
  const timeAt = (clientX: number) => {
    const box = track.current?.getBoundingClientRect();
    if (!box || !known) return 0;
    return Math.min(duration, Math.max(0, ((clientX - box.left) / box.width) * duration));
  };
  const near = hover === null ? undefined : markers.find((marker) => Math.abs(marker.seconds - hover) <= Math.max(1, duration * 0.01));

  return (
    <div className="relative">
      <div
        ref={track}
        role="slider"
        tabIndex={0}
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={Math.floor(duration)}
        aria-valuenow={Math.floor(position)}
        aria-valuetext={`${formatClock(position)} of ${formatClock(duration)}`}
        className="group/track relative flex h-4 cursor-pointer touch-none items-center outline-none"
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          setDragging(true);
          onSeek(timeAt(event.clientX));
        }}
        onPointerMove={(event) => {
          setHover(timeAt(event.clientX));
          if (dragging) onSeek(timeAt(event.clientX));
        }}
        onPointerUp={() => setDragging(false)}
        onPointerLeave={() => setHover(null)}
        onKeyDown={(event) => {
          const step = event.shiftKey ? 30 : 5;
          if (event.key === "ArrowRight") onSeek(Math.min(duration, position + step));
          else if (event.key === "ArrowLeft") onSeek(Math.max(0, position - step));
          else if (event.key === "Home") onSeek(0);
          else if (event.key === "End") onSeek(duration);
          else return;
          event.preventDefault();
          event.stopPropagation();
        }}
      >
        <div className="relative h-1 w-full overflow-hidden rounded-full bg-[#fff]/20 transition-all group-hover/track:h-1.5 group-focus-visible/track:h-1.5">
          {verifiedPercent !== null ? <div className="absolute inset-y-0 left-0 bg-[#6ee7b7]/25" style={{ width: `${verifiedPercent}%` }} /> : null}
          {ranges.map(([start, end], index) => (
            <div key={index} className="absolute inset-y-0 bg-[#fff]/35" style={{ left: percent(start), width: known ? `${((end - start) / duration) * 100}%` : 0 }} />
          ))}
          <div className="absolute inset-y-0 left-0 bg-[#fb7185]" style={{ width: percent(position) }} />
        </div>
        {markers.map((marker, index) => (
          <span key={index} aria-hidden className="pointer-events-none absolute top-1/2 h-2.5 w-1 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#fde68a] shadow" style={{ left: percent(marker.seconds) }} />
        ))}
        <span aria-hidden className="pointer-events-none absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#fff] opacity-0 shadow transition group-hover/track:opacity-100 group-focus-visible/track:opacity-100" style={{ left: percent(position) }} />
      </div>
      {hover !== null && known ? (
        <div className="pointer-events-none absolute bottom-5 -translate-x-1/2 whitespace-nowrap rounded bg-[#000]/85 px-1.5 py-0.5 text-[11px] text-[#fff]" style={{ left: percent(hover) }}>
          {formatClock(hover)}
          {near ? <span className="ml-1 text-[#fde68a]">· {near.label.slice(0, 48)}</span> : null}
        </div>
      ) : null}
    </div>
  );
}
