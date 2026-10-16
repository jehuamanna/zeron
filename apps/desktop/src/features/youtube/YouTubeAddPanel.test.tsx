import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type PlaylistProgress } from "../../services/library";
import { YouTubeAddPanel } from "./YouTubeAddPanel";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: {
    inspectYouTubeLink: vi.fn(),
    addYouTubeVideo: vi.fn(),
    addYouTubePlaylist: vi.fn(),
    cancelYouTubePlaylist: vi.fn(() => Promise.resolve(true)),
    onYouTubePlaylistProgress: vi.fn(),
  },
}));

const PAGE = "https://127.0.0.1:4433/v1/media/h/s#list=PLrust";
const ORIGIN = "https://127.0.0.1:4433";
let progress: ((value: PlaylistProgress) => void) | null = null;

function renderPanel(onOpen = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <YouTubeAddPanel onOpen={onOpen} />
    </QueryClientProvider>,
  );
}

async function paste(view: ReturnType<typeof renderPanel>, link: string) {
  fireEvent.change(view.getByLabelText("YouTube video or playlist link"), { target: { value: link } });
  fireEvent.click(view.getByRole("button", { name: "Add" }));
  return waitFor(() => view.getByTitle("YouTube playlist lister") as HTMLIFrameElement);
}

function from(frame: HTMLIFrameElement, data: unknown, origin = ORIGIN) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, origin, source: frame.contentWindow }));
  });
}

beforeEach(() => {
  vi.mocked(libraryApi.inspectYouTubeLink).mockResolvedValue({ kind: "playlist", playlistId: "PLrust", pageUrl: PAGE });
  vi.mocked(libraryApi.onYouTubePlaylistProgress).mockImplementation((handler) => {
    progress = handler;
    return Promise.resolve(() => {
      progress = null;
    });
  });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("adding from YouTube", () => {
  it("lists a playlist in a hidden frame, shows progress, and reports what was left out", async () => {
    let resolve: (value: Awaited<ReturnType<typeof libraryApi.addYouTubePlaylist>>) => void = () => {};
    vi.mocked(libraryApi.addYouTubePlaylist).mockReturnValue(new Promise((done) => (resolve = done)));
    const onOpen = vi.fn();
    const view = renderPanel(onOpen);
    const frame = await paste(view, "https://www.youtube.com/playlist?list=PLrust");
    expect(frame.getAttribute("src")).toBe(PAGE);
    expect(view.getByRole("status").textContent).toContain("Asking YouTube for the playlist");

    // Messages from elsewhere are ignored.
    from(frame, { zeron: "youtube", type: "playlist", videoIds: ["evil"] }, "https://evil.example");
    expect(libraryApi.addYouTubePlaylist).not.toHaveBeenCalled();

    from(frame, { zeron: "youtube", type: "playlist", videoIds: ["aaaaaaaaaaa", 7, "bbbbbbbbbbb"] });
    await waitFor(() => expect(libraryApi.addYouTubePlaylist).toHaveBeenCalledWith("PLrust", ["aaaaaaaaaaa", "bbbbbbbbbbb"]));
    await waitFor(() => expect(progress).not.toBeNull());
    act(() => progress!({ playlistId: "PLrust", done: 1, total: 2 }));
    expect(view.getByRole("status").textContent).toContain("Checking videos: 1 of 2");

    await act(async () => resolve({ courseId: "youtube-playlist-PLrust", courseVersion: "1.0.0", title: "Rust talks", verifiedResourceCount: 1, totalResourceBytes: 0, skipped: 1 }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Added “Rust talks”: 1 video. 1 private, removed, or blocked video was left out."));
    expect(view.queryByTitle("YouTube playlist lister")).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "Open it" }));
    expect(onOpen).toHaveBeenCalledWith("youtube-playlist-PLrust");
  });

  it("explains a playlist YouTube will not list, and cancels cleanly", async () => {
    const view = renderPanel();
    let frame = await paste(view, "https://www.youtube.com/playlist?list=PLrust");
    from(frame, { zeron: "youtube", type: "playlist", videoIds: [] });
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("YouTube listed no videos"));
    expect(libraryApi.cancelYouTubePlaylist).toHaveBeenCalled();

    frame = await paste(view, "https://www.youtube.com/playlist?list=PLrust");
    from(frame, { zeron: "youtube", type: "error", code: "offline" });
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("could not be reached"));

    await paste(view, "https://www.youtube.com/playlist?list=PLrust");
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    expect(view.queryByTitle("YouTube playlist lister")).toBeNull();
    expect(view.queryByRole("status")).toBeNull();
  });

  it("adds a single video directly", async () => {
    vi.mocked(libraryApi.inspectYouTubeLink).mockResolvedValue({ kind: "video" });
    vi.mocked(libraryApi.addYouTubeVideo).mockResolvedValue({ courseId: "youtube-video-M7lc1UVf-VE", courseVersion: "1.0.0", title: "Player", verifiedResourceCount: 1, totalResourceBytes: 0 });
    const view = renderPanel();
    fireEvent.change(view.getByLabelText("YouTube video or playlist link"), { target: { value: "youtu.be/M7lc1UVf-VE" } });
    fireEvent.click(view.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Added “Player”."));
    expect(libraryApi.addYouTubePlaylist).not.toHaveBeenCalled();
  });
});
