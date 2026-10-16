import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type AnnotationMove, type CourseDetail } from "../../services/library";
import { AnnotationMovesCard, describeReport } from "./AnnotationMovesCard";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { annotationMoves: vi.fn(), moveAnnotations: vi.fn() },
}));

const course = {
  courseId: "course_migrate",
  courseVersion: "1.1.0",
  title: "Systems",
  description: null,
  publisherName: "Zeron Tests",
  source: "local",
  torrentInfoHash: null,
  modules: [{ id: "module", title: "Module", lessons: [
    { id: "lesson_intro", title: "Introduction", resources: [] },
    { id: "lesson_cache", title: "Caching", resources: [] },
  ] }],
} as unknown as CourseDetail;

function item(overrides: Partial<AnnotationMove>): AnnotationMove {
  return { fromLessonId: "lesson_a", fromTitle: "Intro", fromVersion: "1.0.0", suggestedLessonId: "lesson_intro", suggestedTitle: "Introduction", reason: "same_content", notes: 2, notesWidened: 0, hasDrawing: true, hasProgress: false, ...overrides };
}

function renderCard() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><AnnotationMovesCard course={course} /></QueryClientProvider>);
}

beforeEach(() => {
  vi.mocked(libraryApi.annotationMoves).mockResolvedValue([
    item({}),
    item({ fromLessonId: "lesson_c", fromTitle: "Old topic", suggestedLessonId: null, suggestedTitle: null, reason: null, notes: 1, hasDrawing: false }),
  ]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("annotation moves", () => {
  it("shows nothing when every annotation has a place", async () => {
    vi.mocked(libraryApi.annotationMoves).mockResolvedValue([]);
    const view = renderCard();
    await waitFor(() => expect(libraryApi.annotationMoves).toHaveBeenCalledWith("course_migrate"));
    expect(view.container.textContent).toBe("");
  });

  it("explains each suggestion and preselects it", async () => {
    const view = renderCard();
    const list = await waitFor(() => view.getByRole("list", { name: "Annotations to move" }));
    const [first, second] = within(list).getAllByRole("listitem");
    expect(first!.textContent).toContain("“Intro” from version 1.0.0 · 2 notes, a drawing");
    expect(first!.textContent).toContain("Suggested: “Introduction”. Has the same file.");
    expect((within(first!).getByRole("combobox") as HTMLSelectElement).value).toBe("lesson_intro");
    expect(second!.textContent).toContain("No lesson in this version matches");
    expect((within(second!).getByRole("button", { name: /Move here/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("moves to the chosen lesson and reports what changed", async () => {
    vi.mocked(libraryApi.moveAnnotations).mockImplementation(async () => {
      vi.mocked(libraryApi.annotationMoves).mockResolvedValue([]);
      return { notesMoved: 1, notesWidened: 1, drawingMoved: false, drawingKept: false, progressMoved: false, positionsMoved: 0 };
    });
    const view = renderCard();
    const list = await waitFor(() => view.getByRole("list", { name: "Annotations to move" }));
    const second = within(list).getAllByRole("listitem")[1]!;
    fireEvent.change(within(second).getByRole("combobox"), { target: { value: "lesson_cache" } });
    fireEvent.click(within(second).getByRole("button", { name: /Move here/ }));
    await waitFor(() => expect(libraryApi.moveAnnotations).toHaveBeenCalledWith("course_migrate", "lesson_c", "lesson_cache"));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("1 note now points at the whole lesson because its file changed"));
    await waitFor(() => expect(view.queryByRole("list", { name: "Annotations to move" })).toBeNull());
  });

  it("describes kept drawings and moved progress", () => {
    expect(describeReport({ notesMoved: 2, notesWidened: 0, drawingMoved: true, drawingKept: false, progressMoved: true, positionsMoved: 1 })).toBe("Moved 2 notes, the drawing, progress.");
    expect(describeReport({ notesMoved: 0, notesWidened: 0, drawingMoved: false, drawingKept: true, progressMoved: false, positionsMoved: 0 })).toContain("The drawing stayed in its old place");
  });

  it("shows a failed move", async () => {
    vi.mocked(libraryApi.moveAnnotations).mockRejectedValue({ code: "note_target_invalid", message: "the course does not have that lesson" });
    const view = renderCard();
    const list = await waitFor(() => view.getByRole("list", { name: "Annotations to move" }));
    fireEvent.click(within(within(list).getAllByRole("listitem")[0]!).getByRole("button", { name: /Move here/ }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("does not have that lesson"));
  });
});
