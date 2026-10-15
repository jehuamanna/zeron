import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type SearchResult } from "../../services/library";
import { LibrarySearch, Snippet } from "./LibrarySearch";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { search: vi.fn() },
}));

const transcriptHit: SearchResult = {
  kind: "transcript",
  courseId: "course_1",
  courseTitle: "Caching",
  lessonId: "lesson_1",
  lessonTitle: "Eviction",
  sourceId: "video_1",
  position: 3701,
  snippet: "least recently used \u{e000}eviction\u{e001} policy",
};
const noteHit: SearchResult = {
  kind: "note",
  courseId: "course_1",
  courseTitle: "Caching",
  lessonId: "lesson_1",
  lessonTitle: "Eviction",
  sourceId: "note_1",
  position: null,
  snippet: "my \u{e000}eviction\u{e001} notes",
};

function renderSearch() {
  const onOpen = vi.fn();
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <LibrarySearch onOpen={onOpen} />
    </QueryClientProvider>,
  );
  return { view, onOpen, box: view.getByRole("searchbox", { name: "Search your courses" }) };
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("library search", () => {
  it("emphasises matched terms", () => {
    const view = render(<Snippet text={"a \u{e000}b\u{e001} c \u{e000}d\u{e001}"} />);
    expect(Array.from(view.container.querySelectorAll("mark"), (mark) => mark.textContent)).toEqual(["b", "d"]);
    expect(view.container.textContent).toBe("a b c d");
  });

  it("searches after typing pauses and opens a result", async () => {
    vi.mocked(libraryApi.search).mockResolvedValue([transcriptHit, noteHit]);
    const { view, onOpen, box } = renderSearch();
    fireEvent.change(box, { target: { value: "evic" } });
    expect(libraryApi.search).not.toHaveBeenCalled();
    const matches = await waitFor(() => view.getByRole("list", { name: "Matches" }));
    expect(libraryApi.search).toHaveBeenCalledTimes(1);
    expect(libraryApi.search).toHaveBeenCalledWith("evic");
    const [transcript, note] = within(matches).getAllByRole("button");
    expect(transcript?.textContent).toContain("Transcript at 1:01:41 · Caching · Eviction");
    expect(note?.textContent).toContain("Your note · Caching · Eviction");

    fireEvent.click(transcript!);
    expect(onOpen).toHaveBeenCalledWith(transcriptHit);
    expect((box as HTMLInputElement).value).toBe("");
    expect(view.queryByRole("region", { name: "Search results" })).toBeNull();
  });

  it("says when nothing matches and reports failures", async () => {
    vi.mocked(libraryApi.search).mockResolvedValueOnce([]);
    const { view, box } = renderSearch();
    fireEvent.change(box, { target: { value: "sharding" } });
    await waitFor(() => expect(view.getByText("Nothing matches “sharding”.")).toBeTruthy());

    vi.mocked(libraryApi.search).mockRejectedValueOnce({ code: "library_query_failed", message: "the library could not be searched" });
    fireEvent.change(box, { target: { value: "quorum" } });
    await waitFor(() => expect(view.getByRole("alert").textContent).toBe("the library could not be searched"));
  });
});
