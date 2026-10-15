import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LessonPlayer } from "./LessonPlayer";

function setMedia(video: HTMLVideoElement, values: { currentTime?: number; paused?: boolean; error?: { code: number } | null; ranges?: Array<[number, number]> }) {
  if (values.currentTime !== undefined) Object.defineProperty(video, "currentTime", { configurable: true, value: values.currentTime });
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
});
