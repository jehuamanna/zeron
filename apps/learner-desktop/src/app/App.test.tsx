import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../services/library";
import { App } from "./App";
import { useLibraryUi } from "../stores/library";

vi.mock("../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/library")>()),
  libraryApi: {
    readiness: vi.fn(),
    list: vi.fn(),
    chooseAndImport: vi.fn(),
    detail: vi.fn(),
    registerMedia: vi.fn(),
    chooseTorrentFile: vi.fn(),
    importMagnet: vi.fn(),
    getTorrentImport: vi.fn(),
    cancelTorrentImport: vi.fn(),
    onTorrentImport: vi.fn(() => Promise.resolve(() => {})),
    getCourseSharing: vi.fn(() => Promise.resolve(null)),
    getNetworkSettings: vi.fn(),
    availability: vi.fn(() => Promise.resolve([])),
    setNetworkProfile: vi.fn(),
    setCourseSharing: vi.fn(),
  },
}));

function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

describe("learner library", () => {
  beforeEach(() => {
    useLibraryUi.getState().selectCourse(null);
    useLibraryUi.getState().dismissTorrentImport();
    useLibraryUi.getState().showNetwork(false);
  });

  it("shows a durable course summary without a filesystem path", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([
      {
        courseId: "course_one",
        courseVersion: "1.0.0",
        title: "Distributed Systems",
        publisherName: "Zeron Tests",
        language: "en",
        totalResourceBytes: 2_000_000,
        importedAtUnixSeconds: 1,
        source: "local",
        torrentInfoHash: null,
        sharing: false,
      },
    ]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Distributed Systems"));
    expect(view.container.textContent).toContain("Zeron Tests");
    expect(view.container.textContent).not.toContain("/home/");
  });

  it("shows the recoverable empty-library state", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
  });

  it("opens a path-free course outline", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([
      {
        courseId: "course_one",
        courseVersion: "1.0.0",
        title: "Distributed Systems",
        publisherName: "Zeron Tests",
        language: "en",
        totalResourceBytes: 2_000_000,
        importedAtUnixSeconds: 1,
        source: "local",
        torrentInfoHash: null,
        sharing: false,
      },
    ]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one",
      courseVersion: "1.0.0",
      title: "Distributed Systems",
      description: "Course detail",
      publisherName: "Zeron Tests",
      source: "local",
      torrentInfoHash: null,
      modules: [
        {
          id: "module_one",
          title: "Foundations",
          lessons: [
            {
              id: "lesson_one",
              title: "Introduction",
              resources: [
                {
                  id: "resource_video",
                  kind: "video",
                  mediaType: "video/mp4",
                  byteLength: 2_000_000,
                  durationSeconds: 60,
                },
              ],
            },
          ],
        },
      ],
    });

    const view = renderApp();
    const course = await waitFor(() => {
      const button = Array.from(view.container.querySelectorAll("button")).find((candidate) =>
        candidate.textContent?.includes("Distributed Systems"),
      );
      expect(button).toBeDefined();
      return button!;
    });
    fireEvent.click(course);
    await waitFor(() => expect(view.container.textContent).toContain("Foundations"));
    expect(view.container.textContent).toContain("Introduction");
    expect(view.container.textContent).not.toContain("/course/");
  });

  it("requests an opaque URL when a video is selected", async () => {
    useLibraryUi.getState().selectCourse("course_one");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Media Course", description: null,
      publisherName: "Zeron Tests", source: "local", torrentInfoHash: null, modules: [{ id: "module", title: "Module", lessons: [{
        id: "lesson", title: "Lesson", resources: [{ id: "video", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: null }],
      }] }],
    });
    vi.mocked(libraryApi.registerMedia).mockResolvedValue({
      url: "http://127.0.0.1:49152/v1/media/opaque/secret", mediaType: "video/mp4", byteLength: 10,
    });

    const view = renderApp();
    const play = await waitFor(() => {
      const button = Array.from(view.container.querySelectorAll("button")).find((candidate) => candidate.textContent?.includes("Play video"));
      expect(button).toBeDefined();
      return button!;
    });
    fireEvent.click(play);
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/opaque/secret"));
    expect(libraryApi.registerMedia).toHaveBeenCalledWith("course_one", "video");
  });

  it("streams torrent lessons and shows how much of each is downloaded", async () => {
    useLibraryUi.getState().selectCourse("course_torrent");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_torrent", courseVersion: "1.0.0", title: "Torrent Course", description: null,
      publisherName: "Zeron Tests", source: "torrent", torrentInfoHash: "ab".repeat(20), modules: [{ id: "module", title: "Module", lessons: [{
        id: "lesson", title: "Lesson", resources: [{ id: "video", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }],
      }] }],
    });
    vi.mocked(libraryApi.availability).mockResolvedValue([{ resourceId: "video", verifiedBytes: 50, byteLength: 200 }]);
    vi.mocked(libraryApi.registerMedia).mockResolvedValue({
      url: "http://127.0.0.1:49152/v1/media/stream/secret", mediaType: "video/mp4", byteLength: 200,
    });

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Lessons download while you watch"));
    await waitFor(() => expect(view.container.textContent).toContain("25% downloaded"));
    const play = Array.from(view.container.querySelectorAll("button")).find((candidate) => candidate.textContent?.includes("Play video"));
    fireEvent.click(play!);
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/stream/secret"));
    expect(libraryApi.registerMedia).toHaveBeenCalledWith("course_torrent", "video");
    expect(view.container.querySelector('[aria-current="true"]')?.textContent).toContain("Playing");
  });

  it("shows the source of each course in the library", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    const summary = {
      courseVersion: "1.0.0", publisherName: "Zeron Tests", language: "en", totalResourceBytes: 1, importedAtUnixSeconds: 1,
    };
    vi.mocked(libraryApi.list).mockResolvedValue([
      { ...summary, courseId: "local", title: "Local Course", source: "local", torrentInfoHash: null, sharing: false },
      { ...summary, courseId: "torrent", title: "Torrent Course", source: "torrent", torrentInfoHash: "ab".repeat(20), sharing: true },
    ]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Torrent Course"));
    expect(view.container.textContent).toContain("Local · verified");
    expect(view.container.textContent).toContain("Torrent · outline only");
    expect(view.container.textContent).toContain("Sharing");
  });

  it("opens network settings from the header and returns to the library", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue({
      configuredProfile: "local_network",
      activeProfile: "local_network",
      torrentEngine: "ready",
      downloadPort: 1,
      sharingPort: 2,
    });

    const view = renderApp();
    const button = (label: string) =>
      Array.from(view.container.querySelectorAll("button")).find((candidate) => candidate.textContent?.trim() === label);
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
    fireEvent.click(button("Network")!);
    await waitFor(() => expect(view.container.textContent).toContain("Where Zeron looks for torrent courses"));
    expect(view.container.textContent).not.toContain("Your library is ready");
    fireEvent.click(button("Back")!);
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
  });

  it("reports a failed folder import separately from the library and lets it be dismissed", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.chooseAndImport).mockRejectedValue({
      code: "course_import_failed",
      message: "course.json is missing",
    });

    const view = renderApp();
    const screen = within(view.container);
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
    fireEvent.click(screen.getByRole("button", { name: /Import local course/ }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Course import failed");
    expect(alert.textContent).toContain("course.json is missing");
    expect(view.container.textContent).not.toContain("could not open the local library");
    expect(view.container.textContent).toContain("Your library is ready");

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(view.container.textContent).toContain("Your library is ready");
  });

  it("falls back to a generic import message when the failure is not a command error", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.chooseAndImport).mockRejectedValue(new Error("ipc channel closed"));

    const view = renderApp();
    const screen = within(view.container);
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
    fireEvent.click(screen.getByRole("button", { name: /Import local course/ }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The course folder could not be imported.");
    expect(alert.textContent).not.toContain("ipc channel closed");
  });

  it("keeps the library alert for a library that cannot be listed", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockRejectedValue({ code: "library_unavailable", message: "the course library worker is unavailable" });

    const view = renderApp();
    const screen = within(view.container);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Zeron could not open the local library");
    expect(alert.textContent).not.toContain("Course import failed");
  });
});
