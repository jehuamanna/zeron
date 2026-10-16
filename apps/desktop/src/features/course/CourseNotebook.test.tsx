import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type CourseDetail, type Note } from "../../services/library";
import { accessibilityViolations } from "../../test/accessibility";
import { CourseNotebook } from "./CourseNotebook";
import { placeLessons } from "./lessons";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { notes: vi.fn(), drawnLessons: vi.fn() },
}));
// Excalidraw needs a real canvas; the preview's own rendering is not under test here.
vi.mock("../drawings/SketchPreview", () => ({ default: ({ title }: { title: string }) => <p>preview of {title}</p> }));

const course = {
  courseId: "course", courseVersion: "1.0.0", title: "Systems", description: null, publisherName: "P", source: "local", torrentInfoHash: null,
  publisher: { status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null },
  modules: [{ id: "m", title: "Module", lessons: [
    { id: "a", title: "Caching", resources: [{ id: "video_a", kind: "video", mediaType: "video/mp4", byteLength: 1, durationSeconds: 60 }] },
    { id: "b", title: "Sharding", resources: [] },
    { id: "c", title: "Quorums", resources: [] },
  ] }],
} as CourseDetail;

const doc = (text: string) => JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text }] }] });
const note = (noteId: string, lessonId: string, anchor: Note["anchor"], text: string): Note => ({
  noteId, lessonId, anchor, body: doc(text), text, revision: 0, createdAtUnixSeconds: 1, updatedAtUnixSeconds: 1,
});

afterEach(cleanup);

describe("course notebook", () => {
  it("gathers notes and sketches in lesson order, filters them, and jumps to a moment", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([
      note("n3", "b", { kind: "lesson" }, "Shards split by key range"),
      note("n2", "a", { kind: "videoTime", resourceId: "video_a", seconds: 90 }, "Write-through later"),
      note("n1", "a", { kind: "videoTime", resourceId: "video_a", seconds: 30 }, "Write-back first"),
    ]);
    vi.mocked(libraryApi.drawnLessons).mockResolvedValue(["c"]);
    const onSeek = vi.fn();
    const view = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <CourseNotebook courseId="course" placed={placeLessons(course)} onOpenLesson={vi.fn()} onSeek={onSeek} onOpenPage={vi.fn()} onOpenSketch={vi.fn()} onEditNotes={vi.fn()} />
      </QueryClientProvider>,
    );

    const lessons = await waitFor(() => view.getByRole("list", { name: "Lessons with notes" }));
    expect(libraryApi.notes).toHaveBeenCalledWith("course", null);
    expect(within(lessons).getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual(["Caching", "Sharding", "Quorums"]);
    expect(within(view.getByRole("list", { name: "Notes in Caching" })).getAllByText(/Write-/).map((item) => item.textContent)).toEqual(["Write-back first", "Write-through later"]);
    expect(view.container.textContent).toContain("3 notes · 1 sketch");
    await waitFor(() => expect(view.container.textContent).toContain("preview of Quorums"));

    fireEvent.click(view.getByRole("button", { name: "Go to 1:30 in Caching" }));
    expect(onSeek).toHaveBeenCalledWith("video_a", 90);

    fireEvent.change(view.getByRole("searchbox", { name: "Search notes" }), { target: { value: "shard" } });
    expect(within(lessons).getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual(["Sharding"]);

    fireEvent.change(view.getByRole("searchbox", { name: "Search notes" }), { target: { value: "" } });
    fireEvent.click(view.getByRole("button", { name: "sketches" }));
    expect(within(lessons).getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)).toEqual(["Quorums"]);
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });
});
