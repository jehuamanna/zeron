import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePlayerPrefs } from "../../stores/player";
import { AUTOPLAY_SECONDS, LessonPlayer } from "./LessonPlayer";

function setMedia(video: HTMLVideoElement, values: { currentTime?: number; paused?: boolean; duration?: number; error?: { code: number } | null; ranges?: Array<[number, number]> }) {
  if (values.currentTime !== undefined) Object.defineProperty(video, "currentTime", { configurable: true, writable: true, value: values.currentTime });
  if (values.duration !== undefined) Object.defineProperty(video, "duration", { configurable: true, value: values.duration });
  if (values.paused !== undefined) Object.defineProperty(video, "paused", { configurable: true, value: values.paused });
  if (values.error !== undefined) Object.defineProperty(video, "error", { configurable: true, value: values.error });
  if (values.ranges) {
    const ranges = values.ranges;
    Object.defineProperty(video, "buffered", { configurable: true, value: { length: ranges.length, start: (i: number) => ranges[i]![0], end: (i: number) => ranges[i]![1] } });
  }
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("lesson player", () => {
  it("shows loading, playing, buffered and verified amounts, and a stall", () => {
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent verifiedPercent={48} onRetry={vi.fn()} />);
    const video = view.container.querySelector("video")!;
    const status = () => view.getByLabelText("Playback status");
    fireEvent.loadStart(video);
    expect(status().textContent).toBe("Loading…");
    setMedia(video, { currentTime: 0, paused: false, ranges: [[0, 95]] });
    fireEvent.timeUpdate(video);
    setMedia(video, { currentTime: 1.5 });
    fireEvent.timeUpdate(video);
    expect(status().getAttribute("data-state")).toBe("playing");
    expect(view.container.textContent).toContain("Buffered: 1:33 ahead");
    expect(view.container.textContent).toContain("Verified on this computer: 48%");

    fireEvent.waiting(video);
    expect(status().textContent).toBe("Buffering verified data…");
    act(() => vi.advanceTimersByTime(9000));
    expect(status().textContent).toContain("Stalled: waiting for verified data from peers");
    expect(status().getAttribute("role")).toBe("status");
    fireEvent.progress(video);
    setMedia(video, { currentTime: 2.5 });
    fireEvent.timeUpdate(video);
    expect(status().textContent).toBe("Playing");
  });

  it("explains media errors and retries by re-registering the lesson", () => {
    const onRetry = vi.fn();
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent verifiedPercent={0} onRetry={onRetry} />);
    const video = view.container.querySelector("video")!;
    fireEvent.loadStart(video);
    setMedia(video, { error: { code: 4 } });
    fireEvent.error(video);
    const alert = view.getByRole("alert");
    expect(alert.textContent).toBe("This lesson could not be loaded.");
    fireEvent.click(view.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("omits the verified amount for local lessons", () => {
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent={false} verifiedPercent={null} onRetry={vi.fn()} />);
    expect(view.container.textContent).not.toContain("Verified on this computer");
    expect(view.getByLabelText("Playback status").textContent).toBe("Loading…");
  });

  it("resumes from a saved position and can start over", () => {
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent verifiedPercent={10} onRetry={vi.fn()} startAt={125.4} />);
    const video = view.container.querySelector("video")!;
    setMedia(video, { currentTime: 0, duration: 600 });
    fireEvent.loadedMetadata(video);
    expect(video.currentTime).toBe(125.4);
    expect(view.container.textContent).toContain("Resumed where you left off (2:05).");
    fireEvent.click(view.getByRole("button", { name: "Start over" }));
    expect(video.currentTime).toBe(0);
    expect(view.container.textContent).not.toContain("Resumed where you left off");
  });

  it("does not resume from the first seconds or the last seconds", () => {
    for (const startAt of [3, 595]) {
      const view = render(<LessonPlayer url={`https://127.0.0.1:1/v1/media/${startAt}/b`} torrent={false} verifiedPercent={null} onRetry={vi.fn()} startAt={startAt} />);
      const video = view.container.querySelector("video")!;
      setMedia(video, { currentTime: 0, duration: 600 });
      fireEvent.loadedMetadata(video);
      expect(video.currentTime).toBe(0);
      view.unmount();
    }
  });

  it("saves the position while playing at most every few seconds, on pause, at the end, and when closed", () => {
    const onPosition = vi.fn();
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent verifiedPercent={10} onRetry={vi.fn()} onPosition={onPosition} />);
    const video = view.container.querySelector("video")!;
    setMedia(video, { currentTime: 10, duration: 600, paused: false, ranges: [[0, 60]] });
    fireEvent.timeUpdate(video);
    expect(onPosition).toHaveBeenLastCalledWith(10, 600);
    setMedia(video, { currentTime: 11 });
    fireEvent.timeUpdate(video);
    expect(onPosition).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(5000);
    setMedia(video, { currentTime: 16 });
    fireEvent.timeUpdate(video);
    expect(onPosition).toHaveBeenCalledTimes(2);
    setMedia(video, { currentTime: 17, paused: true });
    fireEvent.pause(video);
    expect(onPosition).toHaveBeenLastCalledWith(17, 600);
    setMedia(video, { currentTime: 600 });
    fireEvent.ended(video);
    expect(onPosition).toHaveBeenLastCalledWith(600, 600);
    const calls = onPosition.mock.calls.length;
    view.unmount();
    expect(onPosition).toHaveBeenCalledTimes(calls + 1);
    expect(onPosition).toHaveBeenLastCalledWith(600, 600);
  });
});

describe("lesson player controls", () => {
  beforeEach(() => {
    usePlayerPrefs.setState({ speed: 1, volume: 1, muted: false, captions: false, autoplayNext: true });
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(function (this: HTMLMediaElement) {
      Object.defineProperty(this, "paused", { configurable: true, value: false });
      this.dispatchEvent(new Event("play"));
      return Promise.resolve();
    });
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(function (this: HTMLMediaElement) {
      Object.defineProperty(this, "paused", { configurable: true, value: true });
      this.dispatchEvent(new Event("pause"));
    });
  });

  it("plays and pauses, changes speed and remembers it, and mutes", () => {
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent={false} verifiedPercent={null} onRetry={vi.fn()} />);
    const video = view.container.querySelector("video")!;
    expect(video.hasAttribute("controls")).toBe(false);
    fireEvent.click(view.getByRole("button", { name: "Play" }));
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalled();
    fireEvent.click(view.getByRole("button", { name: "Pause" }));
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: "Playback speed 1×" }));
    fireEvent.click(view.getByRole("menuitemradio", { name: "1.5×" }));
    expect(video.playbackRate).toBe(1.5);
    expect(usePlayerPrefs.getState().speed).toBe(1.5);
    expect(JSON.parse(window.localStorage.getItem("zeron.player")!).speed).toBe(1.5);

    fireEvent.click(view.getByRole("button", { name: "Mute" }));
    expect(video.muted).toBe(true);
    expect(view.getByRole("button", { name: "Unmute" })).toBeTruthy();
  });

  it("shows captions from the transcript and notes on the timeline", () => {
    const cues = [{ start: 0, end: 4, text: "Welcome to caching" }, { start: 4, end: 9, text: "Write-through first" }];
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent={false} verifiedPercent={null} onRetry={vi.fn()} cues={cues} markers={[{ seconds: 5, label: "remember this" }]} />);
    const video = view.container.querySelector("video")!;
    setMedia(video, { duration: 60 });
    fireEvent.durationChange(video);
    setMedia(video, { currentTime: 5, paused: false });
    fireEvent.timeUpdate(video);
    expect(view.container.textContent).not.toContain("Write-through first");
    fireEvent.click(view.getByRole("button", { name: "Show captions" }));
    expect(view.container.textContent).toContain("Write-through first");
    expect(view.getByRole("slider", { name: "Seek" }).getAttribute("aria-valuetext")).toBe("0:05 of 1:00");
  });

  it("counts down to the next lesson at the end, unless cancelled or turned off", () => {
    const onNext = vi.fn();
    const view = render(<LessonPlayer url="https://127.0.0.1:1/v1/media/a/b" torrent={false} verifiedPercent={null} onRetry={vi.fn()} upNext={{ title: "Sharding" }} onNext={onNext} />);
    const video = view.container.querySelector("video")!;
    fireEvent.ended(video);
    expect(view.getByRole("dialog", { name: "Up next" }).textContent).toContain("Sharding");
    expect(view.container.textContent).toContain(`Starting in ${AUTOPLAY_SECONDS}`);
    for (let second = 0; second < AUTOPLAY_SECONDS; second += 1) act(() => vi.advanceTimersByTime(1000));
    expect(onNext).toHaveBeenCalledTimes(1);

    fireEvent.ended(video);
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    act(() => vi.advanceTimersByTime(AUTOPLAY_SECONDS * 1000 + 100));
    expect(onNext).toHaveBeenCalledTimes(1);

    fireEvent.click(view.getByRole("button", { name: "Player settings" }));
    fireEvent.click(view.getByRole("menuitemcheckbox", { name: /Autoplay next lesson/ }));
    expect(usePlayerPrefs.getState().autoplayNext).toBe(false);
    fireEvent.ended(video);
    expect(view.container.textContent).not.toContain("Starting in");
    fireEvent.click(view.getByRole("button", { name: "Play now" }));
    expect(onNext).toHaveBeenCalledTimes(2);
  });
});
