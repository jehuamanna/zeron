import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type Note } from "../../services/library";
import { NotesPanel } from "./NotesPanel";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { notes: vi.fn(), createNote: vi.fn(), updateNote: vi.fn(), deleteNote: vi.fn() },
}));

// The real editor is exercised in NoteEditor.test.tsx; a textarea keeps typing simple here.
vi.mock("./NoteEditor", () => ({
  EMPTY_DOC: '{"type":"doc","content":[{"type":"paragraph"}]}',
  NoteEditor: ({ body, editable, label, onChange }: { body: string; editable: boolean; label: string; onChange?: (json: string, text: string) => void }) => {
    const text = (JSON.parse(body) as { content?: Array<{ content?: Array<{ text?: string }> }> }).content?.[0]?.content?.[0]?.text ?? "";
    return editable ? (
      <textarea
        aria-label={label}
        defaultValue={text}
        onChange={(event) => onChange?.(JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: event.target.value }] }] }), event.target.value)}
      />
    ) : (
      <p>{text}</p>
    );
  },
}));

const doc = (text: string) => JSON.stringify({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text }] }] });
const note = (overrides: Partial<Note>): Note => ({
  noteId: "note_1", lessonId: "lesson_1", anchor: { kind: "videoTime", resourceId: "video_1", seconds: 75 },
  body: doc("Caching tiers"), text: "Caching tiers", revision: 0, createdAtUnixSeconds: 1, updatedAtUnixSeconds: 1, ...overrides,
});

function renderPanel(props: Partial<Parameters<typeof NotesPanel>[0]> = {}) {
  const onSeek = vi.fn();
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <NotesPanel courseId="course_1" lessonId="lesson_1" lessonTitle="Caching" videoResourceId="video_1" currentTime={() => 125.6} onSeek={onSeek} {...props} />
    </QueryClientProvider>,
  );
  return { view, onSeek };
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("lesson notes", () => {
  it("lists notes with their anchors and jumps to a timestamp", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([note({}), note({ noteId: "note_2", anchor: { kind: "lesson" }, body: doc("Overview"), text: "Overview" })]);
    const { view, onSeek } = renderPanel();
    const list = await waitFor(() => view.getByRole("list", { name: "Notes" }));
    expect(list.textContent).toContain("Caching tiers");
    expect(list.textContent).toContain("Overview");
    expect(libraryApi.notes).toHaveBeenCalledWith("course_1", "lesson_1");
    fireEvent.click(within(list).getByRole("button", { name: "Go to 1:15" }));
    expect(onSeek).toHaveBeenCalledWith("video_1", 75);
  });

  it("adds a note at the current time and a note for the whole lesson", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([]);
    vi.mocked(libraryApi.createNote).mockResolvedValue(note({}));
    const { view } = renderPanel();
    await waitFor(() => expect(view.container.textContent).toContain("No notes for this lesson yet."));
    fireEvent.click(view.getByRole("button", { name: /Add note at 2:05/ }));
    const save = view.getByRole("button", { name: "Save note" });
    expect(save.hasAttribute("disabled")).toBe(true);
    fireEvent.change(view.getByLabelText("Note text"), { target: { value: "Write-through cache" } });
    fireEvent.click(save);
    await waitFor(() => expect(libraryApi.createNote).toHaveBeenCalledWith("course_1", "lesson_1", { kind: "videoTime", resourceId: "video_1", seconds: 125 }, doc("Write-through cache")));
    await waitFor(() => expect(view.queryByLabelText("Note text")).toBeNull());

    fireEvent.click(view.getByRole("button", { name: "Add lesson note" }));
    fireEvent.change(view.getByLabelText("Note text"), { target: { value: "Review later" } });
    fireEvent.click(view.getByRole("button", { name: "Save note" }));
    await waitFor(() => expect(libraryApi.createNote).toHaveBeenLastCalledWith("course_1", "lesson_1", { kind: "lesson" }, doc("Review later")));
  });

  it("keeps the learner's text when the note changed elsewhere", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([note({ revision: 2 })]);
    vi.mocked(libraryApi.updateNote).mockResolvedValue({ outcome: "conflict", currentRevision: 3 });
    const { view } = renderPanel();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Edit note" })));
    fireEvent.change(view.getByLabelText("Note text"), { target: { value: "My careful edit" } });
    fireEvent.click(view.getByRole("button", { name: "Save note" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("your edit was not saved"));
    expect(libraryApi.updateNote).toHaveBeenCalledWith("note_1", 2, { kind: "videoTime", resourceId: "video_1", seconds: 75 }, doc("My careful edit"));
    expect((view.getByLabelText("Note text") as HTMLTextAreaElement).value).toBe("My careful edit");
    expect(view.getByRole("button", { name: "Save note" }).hasAttribute("disabled")).toBe(true);
    fireEvent.click(view.getByRole("button", { name: "Load latest" }));
    expect(view.queryByLabelText("Note text")).toBeNull();
    await waitFor(() => expect(libraryApi.notes).toHaveBeenCalledTimes(2));
  });

  it("deletes a note by its revision and offers no timestamp without a video", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([note({ revision: 4 })]);
    vi.mocked(libraryApi.deleteNote).mockResolvedValue({ outcome: "saved", note: note({ revision: 4 }) });
    const { view } = renderPanel({ videoResourceId: null });
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Delete note" })));
    await waitFor(() => expect(libraryApi.deleteNote).toHaveBeenCalledWith("note_1", 4));
    expect(view.queryByRole("button", { name: /Add note at/ })).toBeNull();
  });

  it("adds notes on the open PDF page or its highlighted area", async () => {
    vi.mocked(libraryApi.notes).mockResolvedValue([note({ noteId: "note_pdf", anchor: { kind: "pdfPage", resourceId: "pdf_1", page: 4, region: null } })]);
    vi.mocked(libraryApi.createNote).mockResolvedValue(note({}));
    const onOpenPage = vi.fn();
    const onNoteCreated = vi.fn();
    const region = { x: 0.1, y: 0.2, width: 0.3, height: 0.1 };
    const { view } = renderPanel({ videoResourceId: null, onOpenPage, onNoteCreated, documentAnchor: { kind: "pdfPage", resourceId: "pdf_1", page: 2, region } });
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Go to Page 4" })));
    expect(onOpenPage).toHaveBeenCalledWith("pdf_1", 4);
    fireEvent.click(view.getByRole("button", { name: "Add note on highlight" }));
    fireEvent.change(view.getByLabelText("Note text"), { target: { value: "Consistent hashing diagram" } });
    fireEvent.click(view.getByRole("button", { name: "Save note" }));
    await waitFor(() => expect(libraryApi.createNote).toHaveBeenCalledWith("course_1", "lesson_1", { kind: "pdfPage", resourceId: "pdf_1", page: 2, region }, doc("Consistent hashing diagram")));
    await waitFor(() => expect(onNoteCreated).toHaveBeenCalled());
  });
});
