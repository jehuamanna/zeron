import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../../services/library";
import { activeCueIndex, TranscriptPanel } from "./TranscriptPanel";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { transcript: vi.fn() },
}));

const cues = [
  { start: 1, end: 4, text: "Welcome to caching" },
  { start: 5, end: 9, text: "Write-through caches" },
  { start: 3700, end: 3705, text: "Eviction with LRU" },
];

function renderPanel(time: { now: number }) {
  const onSeek = vi.fn();
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TranscriptPanel courseId="course_1" mediaResourceId="video_1" currentTime={() => time.now} onSeek={onSeek} />
    </QueryClientProvider>,
  );
  return { view, onSeek };
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("lesson transcripts", () => {
  it("finds the cue playing at a time", () => {
    expect([0, 1, 3.9, 4, 5, 3702, 4000].map((time) => activeCueIndex(cues, time))).toEqual([-1, 0, 0, -1, 1, 2, -1]);
    expect(activeCueIndex([], 3)).toBe(-1);
  });

  it("follows the video, seeks from a line, and filters lines", async () => {
    vi.mocked(libraryApi.transcript).mockResolvedValue({ resourceId: "subs", language: "en", cues });
    const time = { now: 6 };
    const { view, onSeek } = renderPanel(time);
    const list = await waitFor(() => view.getByRole("list", { name: "Transcript lines" }));
    expect(libraryApi.transcript).toHaveBeenCalledWith("course_1", "video_1");
    expect(view.getByText("Transcript (en)")).toBeTruthy();
    await waitFor(() => expect(within(list).getByRole("button", { current: true }).textContent).toContain("Write-through"));
    time.now = 3701;
    await waitFor(() => expect(within(list).getByRole("button", { current: true }).textContent).toBe("1:01:40Eviction with LRU"));

    fireEvent.click(within(list).getByText("Welcome to caching"));
    expect(onSeek).toHaveBeenCalledWith(1);

    fireEvent.change(view.getByRole("searchbox", { name: "Search transcript" }), { target: { value: "cach" } });
    expect(within(list).getAllByRole("button").map((button) => button.textContent)).toEqual(["0:01Welcome to caching", "0:05Write-through caches"]);
    fireEvent.change(view.getByRole("searchbox", { name: "Search transcript" }), { target: { value: "sharding" } });
    expect(view.getByText("No line mentions “sharding”.")).toBeTruthy();
  });

  it("shows nothing for media without a transcript and reports unreadable ones", async () => {
    vi.mocked(libraryApi.transcript).mockResolvedValueOnce(null);
    const { view } = renderPanel({ now: 0 });
    await waitFor(() => expect(libraryApi.transcript).toHaveBeenCalled());
    expect(view.queryByRole("region", { name: "Transcript" })).toBeNull();
    cleanup();

    vi.mocked(libraryApi.transcript).mockRejectedValueOnce({ code: "transcript_unreadable", message: "the transcript has no timed lines" });
    const second = renderPanel({ now: 0 });
    await waitFor(() => expect(second.view.getByRole("alert").textContent).toBe("the transcript has no timed lines"));
  });
});
