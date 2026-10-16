import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type Note } from "../../services/library";
import { DocumentViewer } from "./DocumentViewer";
import type { PdfDocumentProps } from "./PdfDocument";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { readDocument: vi.fn(), notes: vi.fn(), createNote: vi.fn(), updateNote: vi.fn(), deleteNote: vi.fn() },
}));

// pdf.js needs a real canvas and layout; this stub reports pages and lets tests select, click, and draw.
vi.mock("./PdfDocument", () => ({
  default: (props: PdfDocumentProps) => {
    useEffect(() => props.onPageCount(3), [props]);
    return (
      <div>
        <p>Showing page {props.page}</p>
        <p data-testid="marks">{props.marks.map((mark) => `${mark.id}:${mark.color ?? "area"}:${mark.style}`).join(",")}</p>
        <p data-testid="layout">{`${props.rotation}|${props.spread}|${props.sidebar ?? "none"}|${props.theme}`}</p>
        <button type="button" onClick={() => props.onSelect({ page: 2, quote: "improve consistency", rects: [{ x: 0.1, y: 0.5, width: 0.6, height: 0.02 }], at: { x: 200, y: 300 } })}>select text</button>
        <button type="button" onClick={() => props.onMark("hl", { x: 100, y: 100 })}>click highlight</button>
        <button type="button" onClick={() => props.onPage(3)}>scroll to 3</button>
        {props.drawing ? <button type="button" onClick={() => props.onRegion(2, { x: 0.1, y: 0.2, width: 0.3, height: 0.1 })}>draw</button> : null}
      </div>
    );
  },
}));

const highlightNote: Note = {
  noteId: "hl",
  lessonId: "lesson_1",
  anchor: { kind: "pdfPage", resourceId: "pdf_1", page: 2, region: null, highlight: { color: "pink", style: "highlight", quote: "a reviewable boundary", rects: [{ x: 0.1, y: 0.6, width: 0.5, height: 0.02 }] } },
  body: "{}",
  text: "a reviewable boundary",
  revision: 3,
  createdAtUnixSeconds: 1,
  updatedAtUnixSeconds: 1,
};
const areaNote: Note = { ...highlightNote, noteId: "area", anchor: { kind: "pdfPage", resourceId: "pdf_1", page: 1, region: { x: 0, y: 0, width: 0.5, height: 0.5 } } };
const otherPdf: Note = { ...highlightNote, noteId: "other", anchor: { ...highlightNote.anchor, resourceId: "pdf_2" } as Note["anchor"] };

function renderViewer(overrides: Partial<Parameters<typeof DocumentViewer>[0]> = {}) {
  const props = { onPage: vi.fn(), onRegion: vi.fn(), onComment: vi.fn(), onEditNote: vi.fn() };
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DocumentViewer courseId="course_1" lessonId="lesson_1" resourceId="pdf_1" title="Handout" page={1} pendingRegion={null} {...props} {...overrides} />
    </QueryClientProvider>,
  );
  return { view, ...props };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(libraryApi.readDocument).mockResolvedValue(new ArrayBuffer(8));
  vi.mocked(libraryApi.notes).mockResolvedValue([highlightNote, areaNote, otherPdf]);
});
afterEach(cleanup);

describe("document viewer", () => {
  it("pages through the document and follows scrolling", async () => {
    const { view, onPage } = renderViewer({ page: 2 });
    await waitFor(() => expect(view.container.textContent).toContain("Page 2 of 3"));
    fireEvent.click(view.getByRole("button", { name: "Next page" }));
    expect(onPage).toHaveBeenCalledWith(3);
    fireEvent.click(view.getByRole("button", { name: "Previous page" }));
    expect(onPage).toHaveBeenCalledWith(1);
    fireEvent.change(view.getByRole("textbox", { name: "Page number" }), { target: { value: "9" } });
    fireEvent.submit(view.getByRole("textbox", { name: "Page number" }));
    expect(onPage).toHaveBeenLastCalledWith(3);
    fireEvent.click(view.getByRole("button", { name: "scroll to 3" }));
    expect(onPage).toHaveBeenLastCalledWith(3);
  });

  it("draws this PDF's highlights and areas, not other files'", async () => {
    const { view } = renderViewer();
    await waitFor(() => expect(view.getByTestId("marks").textContent).toBe("hl:pink:highlight,area:area:highlight"));
  });

  it("highlights selected text in the chosen colour as a note that quotes it", async () => {
    vi.mocked(libraryApi.createNote).mockResolvedValue(highlightNote);
    const { view } = renderViewer();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Use green" })));
    fireEvent.click(view.getByRole("button", { name: "select text" }));
    const toolbar = view.getByRole("toolbar", { name: "Selected text" });
    fireEvent.click(within(toolbar).getByRole("button", { name: "Underline" }));
    await waitFor(() => expect(libraryApi.createNote).toHaveBeenCalled());
    const [course, lesson, anchor, body] = vi.mocked(libraryApi.createNote).mock.calls[0]!;
    expect([course, lesson]).toEqual(["course_1", "lesson_1"]);
    expect(anchor).toEqual({ kind: "pdfPage", resourceId: "pdf_1", page: 2, region: null, highlight: { color: "green", style: "underline", quote: "improve consistency", rects: [{ x: 0.1, y: 0.5, width: 0.6, height: 0.02 }] } });
    expect(JSON.parse(body).content[0]).toMatchObject({ type: "blockquote", content: [{ content: [{ text: "improve consistency" }] }] });
    expect(view.queryByRole("toolbar", { name: "Selected text" })).toBeNull();

    fireEvent.click(view.getByRole("button", { name: "select text" }));
    fireEvent.click(view.getByRole("button", { name: "Highlight purple" }));
    await waitFor(() => expect(libraryApi.createNote).toHaveBeenCalledTimes(2));
    expect((vi.mocked(libraryApi.createNote).mock.calls[1]![2] as { highlight: { color: string; style: string } }).highlight).toMatchObject({ color: "purple", style: "highlight" });
  });

  it("starts a comment on a selection or a page in the Notes panel", async () => {
    const { view, onComment } = renderViewer({ page: 2 });
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "select text" })));
    fireEvent.click(view.getByRole("button", { name: "Highlight and comment" }));
    expect(onComment).toHaveBeenCalledWith(expect.objectContaining({ kind: "pdfPage", page: 2, highlight: expect.objectContaining({ quote: "improve consistency" }) }), expect.stringContaining("blockquote"));
    expect(libraryApi.createNote).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: "Add note on page 2" }));
    expect(onComment).toHaveBeenLastCalledWith({ kind: "pdfPage", resourceId: "pdf_1", page: 2, region: null }, expect.any(String));
  });

  it("recolours, restyles, comments on, and deletes a saved highlight", async () => {
    vi.mocked(libraryApi.updateNote).mockImplementation(async (_id, revision, anchor, body) => ({ outcome: "saved", note: { ...highlightNote, anchor, body, revision: revision + 1 } }));
    vi.mocked(libraryApi.deleteNote).mockResolvedValue({ outcome: "saved", note: highlightNote });
    const { view, onEditNote } = renderViewer();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "click highlight" })));
    let dialog = await waitFor(() => view.getByRole("dialog", { name: "Highlight" }));
    expect(dialog.textContent).toContain("a reviewable boundary");

    fireEvent.click(within(dialog).getByRole("button", { name: "Colour blue" }));
    await waitFor(() => expect(libraryApi.updateNote).toHaveBeenCalledWith("hl", 3, expect.objectContaining({ highlight: expect.objectContaining({ color: "blue", style: "highlight" }) }), "{}"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Strike-through style" }));
    await waitFor(() => expect(libraryApi.updateNote).toHaveBeenLastCalledWith("hl", 4, expect.objectContaining({ highlight: expect.objectContaining({ color: "blue", style: "strike" }) }), "{}"));

    fireEvent.click(within(dialog).getByRole("button", { name: "Comment" }));
    expect(onEditNote).toHaveBeenCalledWith("hl");
    expect(view.queryByRole("dialog", { name: "Highlight" })).toBeNull();

    fireEvent.click(view.getByRole("button", { name: "click highlight" }));
    dialog = view.getByRole("dialog", { name: "Highlight" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(libraryApi.deleteNote).toHaveBeenCalledWith("hl", 3));
  });

  it("boxes an area on a page and shows it until it is cleared", async () => {
    const { view, onRegion } = renderViewer();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /Highlight area/ })));
    fireEvent.click(view.getByRole("button", { name: "draw" }));
    expect(onRegion).toHaveBeenCalledWith(2, { x: 0.1, y: 0.2, width: 0.3, height: 0.1 });

    cleanup();
    const pending = renderViewer({ page: 2, pendingRegion: { x: 0.1, y: 0.2, width: 0.3, height: 0.1 } });
    await waitFor(() => expect(pending.view.container.textContent).toContain("Area boxed on page 2"));
    fireEvent.click(pending.view.getByRole("button", { name: "clear it" }));
    expect(pending.onRegion).toHaveBeenCalledWith(2, null);
  });

  it("opens the sidebar, rotates, shows two pages, and changes page colours", async () => {
    const { view } = renderViewer();
    await waitFor(() => expect(view.getByTestId("layout").textContent).toBe("0|false|none|paper"));
    fireEvent.click(view.getByRole("button", { name: "Show pages and contents" }));
    fireEvent.click(view.getByRole("button", { name: "Rotate pages" }));
    fireEvent.click(view.getByRole("button", { name: "Two pages side by side" }));
    fireEvent.change(view.getByRole("combobox", { name: "Page colours" }), { target: { value: "night" } });
    expect(view.getByTestId("layout").textContent).toBe("90|true|pages|night");
  });

  it("explains documents that cannot be opened", async () => {
    vi.mocked(libraryApi.readDocument).mockRejectedValue({ code: "document_unavailable", message: "the document no longer matches the course" });
    const { view } = renderViewer();
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("no longer matches the course"));
  });
});
