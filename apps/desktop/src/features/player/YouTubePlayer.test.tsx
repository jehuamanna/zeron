import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PlayerHandle } from "./LessonPlayer";
import { describeYouTubeError, YouTubePlayer } from "./YouTubePlayer";

const URL = "https://127.0.0.1:4433/v1/media/handle/secret#v=M7lc1UVf-VE";
const ORIGIN = "https://127.0.0.1:4433";

afterEach(cleanup);

function from(frame: HTMLIFrameElement, data: unknown, origin = ORIGIN, source: MessageEventSource | null = frame.contentWindow) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, origin, source }));
  });
}

describe("YouTube player", () => {
  it("frames the gateway page, follows its reports, and saves the position", () => {
    const onPosition = vi.fn();
    const view = render(<YouTubePlayer url={URL} onRetry={vi.fn()} onPosition={onPosition} />);
    const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
    expect(frame.getAttribute("src")).toBe(URL);
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
    const status = () => view.getByLabelText("Playback status");
    expect(status().textContent).toBe("Loading YouTube…");

    from(frame, { zeron: "youtube", type: "ready", duration: 1344 });
    from(frame, { zeron: "youtube", type: "time", position: 61, duration: 1344, state: 1 });
    expect(status().getAttribute("data-state")).toBe("playing");
    expect(view.container.textContent).toContain("1:01 / 22:24");
    expect(onPosition).toHaveBeenLastCalledWith(61, 1344);

    from(frame, { zeron: "youtube", type: "time", position: 62, duration: 1344, state: 2 });
    expect(status().textContent).toBe("Paused · YouTube");
    expect(onPosition).toHaveBeenLastCalledWith(62, 1344);
    view.unmount();
    expect(onPosition).toHaveBeenLastCalledWith(62, 1344);
  });

  it("ignores messages from other windows or origins", () => {
    const view = render(<YouTubePlayer url={URL} onRetry={vi.fn()} />);
    const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
    const playing = { zeron: "youtube", type: "time", position: 5, duration: 10, state: 1 };
    from(frame, playing, "https://evil.example");
    from(frame, playing, ORIGIN, window);
    from(frame, { type: "time", position: 5, duration: 10, state: 1 });
    expect(view.getByLabelText("Playback status").getAttribute("data-state")).toBe("loading");
  });

  it("drives the player through messages to the gateway origin only", () => {
    const handle = createRef<PlayerHandle>();
    const view = render(<YouTubePlayer handle={handle} url={URL} onRetry={vi.fn()} />);
    const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
    const post = vi.spyOn(frame.contentWindow!, "postMessage");
    from(frame, { zeron: "youtube", type: "time", position: 30, duration: 100, state: 1 });

    expect(handle.current!.currentTime()).toBe(30);
    act(() => handle.current!.skip(-10));
    expect(post).toHaveBeenLastCalledWith({ zeron: "youtube", type: "seek", seconds: 20 }, ORIGIN);
    act(() => handle.current!.seek(90));
    expect(post).toHaveBeenCalledWith({ zeron: "youtube", type: "seek", seconds: 90 }, ORIGIN);
    expect(post).toHaveBeenLastCalledWith({ zeron: "youtube", type: "play" }, ORIGIN);
    handle.current!.togglePlay();
    expect(post).toHaveBeenLastCalledWith({ zeron: "youtube", type: "toggle" }, ORIGIN);
    handle.current!.nudgeVolume(0.1);
    expect(post).toHaveBeenLastCalledWith({ zeron: "youtube", type: "volume", step: 0.1 }, ORIGIN);
  });

  it("resumes from a saved position, marks notes, and adds notes at the moment", () => {
    const onAddNote = vi.fn();
    const view = render(
      <YouTubePlayer url={URL} onRetry={vi.fn()} startAt={95.6} onAddNote={onAddNote} markers={[{ seconds: 50, label: "Origin rules" }]} />,
    );
    const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
    expect(frame.getAttribute("src")).toBe(`${URL}&t=95`);
    const post = vi.spyOn(frame.contentWindow!, "postMessage");
    from(frame, { zeron: "youtube", type: "time", position: 96, duration: 100, state: 1 });

    fireEvent.click(view.getByRole("button", { name: /Note at 1:36/ }));
    expect(onAddNote).toHaveBeenCalledWith(96);
    fireEvent.click(view.getByRole("button", { name: "Go to the note at 0:50: Origin rules" }));
    expect(post).toHaveBeenCalledWith({ zeron: "youtube", type: "seek", seconds: 50 }, ORIGIN);
  });

  it("explains why YouTube refused and offers a retry", () => {
    const onRetry = vi.fn();
    const view = render(<YouTubePlayer url={URL} onRetry={onRetry} />);
    const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
    from(frame, { zeron: "youtube", type: "error", code: 150 });
    expect(view.getByRole("alert").textContent).toBe("The owner of this video does not allow it to be played outside YouTube.");
    fireEvent.click(view.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
    expect(describeYouTubeError("offline")).toContain("could not be reached");
    expect(describeYouTubeError(100)).toContain("unavailable");
  });

  it("counts down to the next lesson when a video ends", () => {
    vi.useFakeTimers();
    try {
      const onNext = vi.fn();
      const view = render(<YouTubePlayer url={URL} onRetry={vi.fn()} upNext={{ title: "Part 2" }} onNext={onNext} />);
      const frame = view.getByTitle("YouTube video player") as HTMLIFrameElement;
      from(frame, { zeron: "youtube", type: "time", position: 99, duration: 100, state: 0 });
      expect(view.getByRole("dialog", { name: "Up next" }).textContent).toContain("Part 2");
      expect(view.getByRole("dialog", { name: "Up next" }).textContent).toContain("Starting in 5");
      for (let second = 0; second < 6; second += 1) act(() => vi.advanceTimersByTime(1000));
      expect(onNext).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});
