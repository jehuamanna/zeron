import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
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
    addYouTubeVideo: vi.fn(),
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
    drawnLessons: vi.fn(() => Promise.resolve([])),
    removeCourse: vi.fn(),
    removedCourses: vi.fn(() => Promise.resolve([])),
    storage: vi.fn(() => Promise.resolve({ streamedBytes: 0, cacheLimitBytes: null, availableBytes: 50 * 1024 ** 3, lowDisk: false })),
    savePlaybackPosition: vi.fn(() => Promise.resolve(true)),
    search: vi.fn(),
    importLicense: vi.fn(),
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

const lockedCourse = {
  courseId: "course_paid",
  courseVersion: "1.0.0",
  title: "Paid Course",
  description: null,
  publisherName: "Zeron Tests",
  source: "local" as const,
  torrentInfoHash: null,
  publisher: { status: "unsigned" as const, fingerprint: null, publisherId: null, signedAtUnixSeconds: null },
  paid: true,
  paidUnlocked: false,
  modules: [
    {
      id: "module",
      title: "Module",
      lessons: [
        {
          id: "preview",
          title: "Free preview",
          paid: false,
          locked: false,
          resources: [{ id: "preview_video", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: null, locked: false }],
        },
        {
          id: "paid",
          title: "Deep dive",
          paid: true,
          locked: true,
          resources: [{ id: "paid_video", kind: "video", mediaType: "video/mp4", byteLength: 10, durationSeconds: null, locked: true }],
        },
      ],
    },
  ],
};

describe("paid lessons", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useLibraryUi.getState().selectCourse("course_paid");
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready", torrentImport: "ready", sharing: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue(lockedCourse);
    vi.mocked(libraryApi.registerMedia).mockResolvedValue({
      url: "http://127.0.0.1:49152/v1/media/opaque/secret", mediaType: "video/mp4", byteLength: 10,
    });
  });

  it("marks a locked lesson and explains it instead of loading it", async () => {
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Deep dive"));
    expect(view.getAllByText("Paid lesson, locked")).toHaveLength(1);
    expect(view.getAllByText("Play video")).toHaveLength(1);

    const lesson = Array.from(view.container.querySelectorAll("button")).find((button) => button.getAttribute("title") === "Deep dive");
    fireEvent.click(lesson!);
    await waitFor(() => expect(view.container.textContent).toContain("Deep dive is a paid lesson"));
    expect(view.container.textContent).toContain("free preview lessons stay open");
    expect(libraryApi.registerMedia).not.toHaveBeenCalled();
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });

  it("imports a license from the locked lesson and reloads the course", async () => {
    const unlocked = {
      ...lockedCourse,
      paidUnlocked: true,
      modules: lockedCourse.modules.map((module) => ({
        ...module,
        lessons: module.lessons.map((lesson) => ({ ...lesson, locked: false, resources: lesson.resources.map((resource) => ({ ...resource, locked: false })) })),
      })),
    };
    vi.mocked(libraryApi.importLicense)
      .mockRejectedValueOnce({ code: "license_invalid", message: "this license cannot be used: the signature is for another learner" })
      .mockImplementationOnce(async () => {
        vi.mocked(libraryApi.detail).mockResolvedValue(unlocked);
        return { courseId: "course_paid", courseTitle: "Paid Course", versions: ["1.0.0"], unlocksCurrent: true, alreadyImported: false };
      });
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Deep dive"));
    const lesson = Array.from(view.container.querySelectorAll("button")).find((button) => button.getAttribute("title") === "Deep dive");
    fireEvent.click(lesson!);
    fireEvent.click(await view.findByRole("button", { name: /Import license/ }));
    expect((await view.findByRole("alert")).textContent).toContain("another learner");
    fireEvent.click(view.getByRole("button", { name: /Import license/ }));
    await waitFor(() => expect(view.queryAllByText("Paid lesson, locked")).toHaveLength(0));
    expect(view.getAllByText("Play video")).toHaveLength(2);
  });

  it("still plays the free preview", async () => {
    const view = renderApp();
    const play = await waitFor(() => {
      const button = Array.from(view.container.querySelectorAll("button")).find((candidate) => candidate.textContent?.includes("Play video"));
      expect(button).toBeDefined();
      return button!;
    });
    fireEvent.click(play);
    await waitFor(() => expect(view.container.querySelector("video")?.src).toContain("/v1/media/opaque/secret"));
    expect(libraryApi.registerMedia).toHaveBeenCalledWith("course_paid", "preview_video");
  });
});
