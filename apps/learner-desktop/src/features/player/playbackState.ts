/**
 * Lesson playback states derived from media events.
 *
 * WebKitGTK keeps `readyState` low and fires `stalled` while a stream plays
 * normally, so playing is judged by `currentTime` advancing and stalled by a
 * watchdog: buffering with no progress for `STALL_AFTER_MS`.
 */
export type PlaybackState = "loading" | "buffering" | "playing" | "paused" | "stalled" | "ended" | "error";

export interface Playback {
  state: PlaybackState;
  /** Last position seen, in seconds. */
  position: number;
  /** Media error code (1–4) when `state` is `error`. */
  errorCode: number | null;
  /** When the current wait started, for the stall watchdog. */
  waitingSince: number | null;
}

export type PlaybackEvent =
  | { type: "loadstart"; at: number }
  | { type: "waiting"; at: number }
  | { type: "progress"; at: number }
  | { type: "seeking"; at: number }
  | { type: "timeupdate"; position: number; paused: boolean; at: number }
  | { type: "playing"; at: number }
  | { type: "pause"; at: number }
  | { type: "ended" }
  | { type: "error"; code: number }
  | { type: "tick"; at: number };

export const STALL_AFTER_MS = 8000;

export const initialPlayback: Playback = { state: "loading", position: 0, errorCode: null, waitingSince: null };

export function playbackReducer(current: Playback, event: PlaybackEvent): Playback {
  if (current.state === "error" && event.type !== "loadstart") return current;
  switch (event.type) {
    case "loadstart":
      return { ...initialPlayback, waitingSince: event.at };
    case "waiting":
      return { ...current, state: current.state === "loading" ? "loading" : "buffering", waitingSince: event.at };
    case "seeking":
      // The jump to the new position must not count as playback progress.
      return { ...current, state: current.state === "loading" ? "loading" : "buffering", waitingSince: event.at, position: Number.POSITIVE_INFINITY };
    case "progress":
      return current.waitingSince === null ? current : { ...current, waitingSince: event.at, state: current.state === "stalled" ? "buffering" : current.state };
    case "timeupdate": {
      if (event.paused) return { ...current, position: event.position };
      if (!Number.isFinite(current.position)) return { ...current, position: event.position };
      const advanced = event.position > current.position + 0.05;
      return advanced
        ? { ...current, state: "playing", position: event.position, waitingSince: null }
        : { ...current, position: event.position };
    }
    case "playing":
      return current.state === "loading" ? { ...current, waitingSince: event.at } : current;
    case "pause":
      return current.state === "ended" ? current : { ...current, state: "paused", waitingSince: null };
    case "ended":
      return { ...current, state: "ended", waitingSince: null };
    case "error":
      return { ...current, state: "error", errorCode: event.code, waitingSince: null };
    case "tick":
      return (current.state === "buffering" || current.state === "loading") &&
        current.waitingSince !== null &&
        event.at - current.waitingSince >= STALL_AFTER_MS
        ? { ...current, state: "stalled" }
        : current;
  }
}

export function describePlayback(playback: Playback, torrent: boolean): string {
  switch (playback.state) {
    case "loading":
      return "Loading…";
    case "buffering":
      return torrent ? "Buffering verified data…" : "Buffering…";
    case "playing":
      return "Playing";
    case "paused":
      return "Paused";
    case "ended":
      return "Finished";
    case "stalled":
      return torrent
        ? "Stalled: waiting for verified data from peers. It resumes on its own when data arrives."
        : "Stalled: the lesson file is not being read.";
    case "error":
      return mediaErrorMessage(playback.errorCode);
  }
}

export function mediaErrorMessage(code: number | null): string {
  switch (code) {
    case 1:
      return "Playback was stopped.";
    case 2:
      return "The lesson stopped loading.";
    case 3:
      return "This video could not be decoded on this computer.";
    default:
      return "This lesson could not be loaded.";
  }
}
