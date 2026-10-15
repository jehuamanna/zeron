import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../services/library";
import { App } from "./App";
import { useLibraryUi } from "../stores/library";

vi.mock("../services/library", () => ({
  libraryApi: {
    readiness: vi.fn(),
    list: vi.fn(),
    chooseAndImport: vi.fn(),
    detail: vi.fn(),
    registerMedia: vi.fn(),
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

describe("learner library", () => {
  beforeEach(() => useLibraryUi.getState().selectCourse(null));

  it("shows a durable course summary without a filesystem path", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([
      {
        courseId: "course_one",
        courseVersion: "1.0.0",
        title: "Distributed Systems",
        publisherName: "Zeron Tests",
        language: "en",
        totalResourceBytes: 2_000_000,
        importedAtUnixSeconds: 1,
      },
    ]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Distributed Systems"));
    expect(view.container.textContent).toContain("Zeron Tests");
    expect(view.container.textContent).not.toContain("/home/");
  });

  it("shows the recoverable empty-library state", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);

    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Your library is ready"));
  });

  it("opens a path-free course outline", async () => {
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([
      {
        courseId: "course_one",
        courseVersion: "1.0.0",
        title: "Distributed Systems",
        publisherName: "Zeron Tests",
        language: "en",
        totalResourceBytes: 2_000_000,
        importedAtUnixSeconds: 1,
      },
    ]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one",
      courseVersion: "1.0.0",
      title: "Distributed Systems",
      description: "Course detail",
      publisherName: "Zeron Tests",
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
    vi.mocked(libraryApi.readiness).mockResolvedValue({ state: "ready" });
    vi.mocked(libraryApi.list).mockResolvedValue([]);
    vi.mocked(libraryApi.detail).mockResolvedValue({
      courseId: "course_one", courseVersion: "1.0.0", title: "Media Course", description: null,
      publisherName: "Zeron Tests", modules: [{ id: "module", title: "Module", lessons: [{
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
});
