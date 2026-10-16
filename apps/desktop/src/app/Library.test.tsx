import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../services/library";
import { Library } from "./Library";
import { useLibraryUi } from "../stores/library";
import { accessibilityViolations } from "../test/accessibility";

vi.mock("../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/library")>()),
  libraryApi: {
    listCatalogs: vi.fn(() => Promise.resolve([])),
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
    playbackPosition: vi.fn(() => Promise.resolve(null)),
    downloads: vi.fn(() => Promise.resolve({ paused: false, course: false, modules: [], lessons: [] })),
    setDownload: vi.fn(),
    setPaused: vi.fn(),
    removeDownloads: vi.fn(),
    progress: vi.fn(() => Promise.resolve({ completedLessons: 0, totalLessons: 0, percent: 0, lessons: [] })),
    setLessonCompleted: vi.fn(),
    notes: vi.fn(() => Promise.resolve([])),
    storage: vi.fn(() => Promise.resolve({ streamedBytes: 0, cacheLimitBytes: null, availableBytes: 50 * 1024 ** 3, lowDisk: false })),
    savePlaybackPosition: vi.fn(() => Promise.resolve(true)),
    search: vi.fn(),
  },
}));

function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <Library />
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

describe("learner library", () => {
  beforeEach(() => {
    useLibraryUi.getState().selectCourse(null);
    useLibraryUi.getState().dismissTorrentImport();
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
        completedLessons: 0,
        totalLessons: 1,
      },
    ]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Distributed Systems"));
    expect(view.container.textContent).toContain("Zeron Tests");
    expect(view.container.textContent).not.toContain("/home/");
    expect(await accessibilityViolations(view.container)).toEqual([]);
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
        completedLessons: 0,
        totalLessons: 1,
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
      publisher: { status: "verified", fingerprint: "7F3A-91C2-0B44-E8D1", publisherId: `ed25519-${"a".repeat(64)}`, signedAtUnixSeconds: 1 },
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
    expect(view.container.textContent).toContain("Verified publisher");
    expect(view.container.textContent).toContain("7F3A-91C2-0B44-E8D1");
    expect(view.container.textContent).toContain("Introduction");
    expect(view.container.textContent).not.toContain("/course/");
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });

  it("requests an opaque URL when a video is selected", async () => {
    useLibraryUi.getState().selectCourse("course_one");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Media Course", description: null,
      publisherName: "Zeron Tests", source: "local", torrentInfoHash: null, publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [{
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
    expect(libraryApi.playbackPosition).toHaveBeenCalledWith("course_one", "video");
  });

  it("opens a transcript search match in its course at the matched moment", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Media Course", description: null,
      publisherName: "Zeron Tests", source: "local", torrentInfoHash: null, publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [{
        id: "lesson", title: "Lesson", resources: [{ id: "video", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: null }],
      }] }],
    });
    vi.mocked(libraryApi.registerMedia).mockResolvedValue({
      url: "http://127.0.0.1:49152/v1/media/opaque/secret", mediaType: "video/mp4", byteLength: 10,
    });
    vi.mocked(libraryApi.search).mockResolvedValue([{
      kind: "transcript", courseId: "course_one", courseTitle: "Media Course", lessonId: "lesson", lessonTitle: "Lesson",
      sourceId: "video", position: 42, snippet: "\u{e000}quorum\u{e001} reads",
    }]);

    const view = renderApp();
    fireEvent.change(await waitFor(() => view.getByRole("searchbox", { name: "Search your courses" })), { target: { value: "quorum" } });
    const matches = await waitFor(() => view.getByRole("list", { name: "Matches" }));
    fireEvent.click(within(matches).getByRole("button"));
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/opaque/secret"));
    expect(useLibraryUi.getState().selectedCourseId).toBe("course_one");
    expect(libraryApi.registerMedia).toHaveBeenCalledWith("course_one", "video");
  });

  it("streams torrent lessons and shows how much of each is downloaded", async () => {
    useLibraryUi.getState().selectCourse("course_torrent");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_torrent", courseVersion: "1.0.0", title: "Torrent Course", description: null,
      publisherName: "Zeron Tests", source: "torrent", torrentInfoHash: "ab".repeat(20), publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [{
        id: "lesson", title: "Lesson", resources: [{ id: "video", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }],
      }] }],
    });
    vi.mocked(libraryApi.availability).mockResolvedValue([{ resourceId: "video", verifiedBytes: 50, byteLength: 200, kept: false, state: "partially_available" }]);
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
    expect(view.container.querySelector("[data-availability]")?.textContent).toBe("Partly on this computer");
    expect(view.queryByRole("button", { name: "Remove download of Lesson" })).toBeNull();
  });

  it("removes streamed lessons that are not kept, one at a time or for the whole course", async () => {
    useLibraryUi.getState().selectCourse("course_torrent");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_torrent", courseVersion: "1.0.0", title: "Torrent Course", description: null,
      publisherName: "Zeron Tests", source: "torrent", torrentInfoHash: "ab".repeat(20), publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [
        { id: "lesson_a", title: "First", resources: [{ id: "video_a", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }] },
        { id: "lesson_b", title: "Second", resources: [{ id: "video_b", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }] },
      ] }],
    });
    vi.mocked(libraryApi.availability).mockResolvedValue([
      { resourceId: "video_a", verifiedBytes: 120, byteLength: 200, kept: false, state: "partially_available" },
      { resourceId: "video_b", verifiedBytes: 200, byteLength: 200, kept: true, state: "available_offline" },
    ]);
    vi.mocked(libraryApi.removeDownloads).mockResolvedValue({ removed: ["video_a"], freedBytes: 5 * 1024 ** 2, kept: [] });
    const view = renderApp();
    const remove = await waitFor(() => view.getByRole("button", { name: "Remove download of First" }));
    expect(view.queryByRole("button", { name: "Remove download of Second" })).toBeNull();
    fireEvent.click(remove);
    await waitFor(() => expect(libraryApi.removeDownloads).toHaveBeenCalledWith("course_torrent", ["video_a"]));
    fireEvent.click(view.getByRole("button", { name: "Free up space" }));
    await waitFor(() => expect(libraryApi.removeDownloads).toHaveBeenLastCalledWith("course_torrent", undefined));
    await waitFor(() => expect(view.container.textContent).toContain("Freed 5 MB. Lessons kept offline were not touched."));
  });

  it("explains that downloads pause when the disk is nearly full", async () => {
    useLibraryUi.getState().selectCourse("course_torrent");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_torrent", courseVersion: "1.0.0", title: "Torrent Course", description: null,
      publisherName: "Zeron Tests", source: "torrent", torrentInfoHash: "ab".repeat(20), publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [],
    });
    vi.mocked(libraryApi.storage).mockResolvedValue({ streamedBytes: 0, cacheLimitBytes: null, availableBytes: 200 * 1024 ** 2, lowDisk: true });
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Downloads are paused because less than 1 GB is free"));
  });

  it("keeps lessons, modules, and whole courses offline and labels their availability", async () => {
    useLibraryUi.getState().selectCourse("course_torrent");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_torrent", courseVersion: "1.0.0", title: "Torrent Course", description: null,
      publisherName: "Zeron Tests", source: "torrent", torrentInfoHash: "ab".repeat(20), publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [
        { id: "lesson_a", title: "First", resources: [{ id: "video_a", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }] },
        { id: "lesson_b", title: "Second", resources: [{ id: "video_b", kind: "video", mediaType: "video/mp4", byteLength: 200, durationSeconds: null }] },
      ] }],
    });
    let kept = { paused: false, course: false, modules: [] as string[], lessons: [] as string[] };
    vi.mocked(libraryApi.downloads).mockImplementation(async () => kept);
    vi.mocked(libraryApi.availability).mockImplementation(async () => [
      { resourceId: "video_a", verifiedBytes: kept.lessons.includes("lesson_a") || kept.course ? 200 : 0, byteLength: 200, kept: kept.lessons.includes("lesson_a") || kept.course, state: kept.lessons.includes("lesson_a") || kept.course ? "available_offline" : "online_only" },
      { resourceId: "video_b", verifiedBytes: 80, byteLength: 200, kept: kept.course, state: kept.course ? "downloading" : "partially_available" },
    ]);
    vi.mocked(libraryApi.setDownload).mockImplementation(async (_course, scope, keep) => {
      if (scope.kind === "course") kept = { ...kept, course: keep };
      if (scope.kind === "lesson") kept = { ...kept, lessons: keep ? [...kept.lessons, scope.id] : kept.lessons.filter((id) => id !== scope.id) };
      return kept;
    });

    const view = renderApp();
    const lessonToggle = await waitFor(() => view.getByRole("button", { name: "Keep lesson First offline" }));
    const labels = () => Array.from(view.container.querySelectorAll("[data-availability]")).map((label) => label.textContent);
    await waitFor(() => expect(labels()).toEqual(["Online only", "Partly on this computer"]));
    expect(lessonToggle.getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(lessonToggle);
    await waitFor(() => expect(libraryApi.setDownload).toHaveBeenCalledWith("course_torrent", { kind: "lesson", id: "lesson_a" }, true));
    await waitFor(() => expect(view.getByRole("button", { name: "Keep lesson First offline" }).getAttribute("aria-pressed")).toBe("true"));
    await waitFor(() => expect(labels()).toEqual(["Available offline", "Partly on this computer"]));

    fireEvent.click(view.getByRole("button", { name: "Keep the whole course offline" }));
    await waitFor(() => expect(libraryApi.setDownload).toHaveBeenCalledWith("course_torrent", { kind: "course" }, true));
    await waitFor(() => expect(view.getByRole("button", { name: "Keep module Module offline" }).hasAttribute("disabled")).toBe(true));
    expect(view.getByRole("button", { name: "Keep lesson Second offline" }).getAttribute("aria-pressed")).toBe("true");
    await waitFor(() => expect(labels()).toEqual(["Available offline", "Downloading for offline"]));

    vi.mocked(libraryApi.setPaused).mockImplementation(async (_course, paused) => {
      kept = { ...kept, paused };
      return kept;
    });
    fireEvent.click(view.getByRole("button", { name: "Pause downloads" }));
    await waitFor(() => expect(libraryApi.setPaused).toHaveBeenCalledWith("course_torrent", true));
    await waitFor(() => expect(view.container.textContent).toContain("Downloads and sharing are paused"));
    const resume = view.getByRole("button", { name: "Resume downloads" });
    expect(resume.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(resume);
    await waitFor(() => expect(libraryApi.setPaused).toHaveBeenLastCalledWith("course_torrent", false));
    await waitFor(() => expect(view.container.textContent).not.toContain("Downloads and sharing are paused"));
  });

  it("shows the source of each course in the library", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    const summary = {
      courseVersion: "1.0.0", publisherName: "Zeron Tests", language: "en", totalResourceBytes: 1, importedAtUnixSeconds: 1,
      completedLessons: 3, totalLessons: 13,
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
    expect(view.getByRole("progressbar", { name: "Torrent Course progress" }).getAttribute("aria-valuenow")).toBe("3");
    expect(view.container.textContent).toContain("3 of 13 lessons complete");
  });

  it("shows lesson progress and lets the learner mark lessons complete", async () => {
    useLibraryUi.getState().selectCourse("course_one");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Course", description: null,
      publisherName: "Zeron Tests", source: "local", torrentInfoHash: null, publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [
        { id: "lesson_a", title: "First", resources: [] },
        { id: "lesson_b", title: "Second", resources: [] },
      ] }],
    });
    vi.mocked(libraryApi.progress).mockResolvedValue({
      completedLessons: 1, totalLessons: 2, percent: 50,
      lessons: [{ lessonId: "lesson_a", status: "completed" }, { lessonId: "lesson_b", status: "in_progress" }],
    });
    vi.mocked(libraryApi.setLessonCompleted).mockResolvedValue({
      completedLessons: 2, totalLessons: 2, percent: 100,
      lessons: [{ lessonId: "lesson_a", status: "completed" }, { lessonId: "lesson_b", status: "completed" }],
    });
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("1 of 2 lessons complete"));
    const statuses = () => Array.from(view.container.querySelectorAll("[data-lesson-status]")).map((item) => item.getAttribute("data-lesson-status"));
    expect(statuses()).toEqual(["completed", "in_progress"]);
    expect(view.getByRole("button", { name: "Mark First complete" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(view.getByRole("button", { name: "Mark Second complete" }));
    await waitFor(() => expect(libraryApi.setLessonCompleted).toHaveBeenCalledWith("course_one", "lesson_b", true));
    await waitFor(() => expect(view.container.textContent).toContain("2 of 2 lessons complete"));
    expect(statuses()).toEqual(["completed", "completed"]);

    fireEvent.click(view.getByRole("button", { name: "Notes for Second" }));
    await waitFor(() => expect(view.getByRole("region", { name: "Notes for Second" })).toBeTruthy());
    expect(libraryApi.notes).toHaveBeenCalledWith("course_one", "lesson_b");
    expect(view.queryByRole("button", { name: /Add note at/ })).toBeNull();
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

  it("opens a lesson in the workspace, steps to the next one, and toggles panes from the keyboard", async () => {
    useLibraryUi.getState().selectCourse("course_one");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Course", description: null,
      publisherName: "Zeron Tests", source: "local", torrentInfoHash: null, publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null }, modules: [{ id: "module", title: "Module", lessons: [
        { id: "lesson_a", title: "First", resources: [{ id: "video_a", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: 60 }] },
        { id: "lesson_b", title: "Second", resources: [{ id: "video_b", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: 90 }] },
      ] }],
    });
    vi.mocked(libraryApi.progress).mockResolvedValue({ completedLessons: 0, totalLessons: 2, percent: 0, lessons: [] });
    vi.mocked(libraryApi.registerMedia).mockImplementation(async (_course, resourceId) => ({ url: `http://127.0.0.1:1/v1/media/${resourceId}`, mediaType: "video/mp4", byteLength: 10 }));

    const view = renderApp();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Start course" })));
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/video_a"));
    expect(view.getByRole("heading", { level: 1 }).textContent).toBe("First");
    expect(view.container.textContent).toContain("Lesson 1 of 2");
    expect(view.getByRole("button", { name: "Previous lesson" }).hasAttribute("disabled")).toBe(true);

    fireEvent.click(view.getByRole("button", { name: "Next lesson" }));
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/video_b"));
    expect(view.container.textContent).toContain("Lesson 2 of 2");

    expect(view.getByRole("complementary", { name: "Course contents" })).toBeTruthy();
    fireEvent.keyDown(document.body, { key: "[" });
    expect(view.queryByRole("complementary", { name: "Course contents" })).toBeNull();
    fireEvent.keyDown(document.body, { key: "[" });
    expect(view.getByRole("complementary", { name: "Course contents" })).toBeTruthy();

    fireEvent.keyDown(document.body, { key: "n" });
    await waitFor(() => expect(view.getByRole("region", { name: "Notes for Second" }).textContent).toContain("New note · 0:00"));
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });
});
