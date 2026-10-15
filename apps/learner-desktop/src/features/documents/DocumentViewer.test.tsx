import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type Region } from "../../services/library";
import { DocumentViewer } from "./DocumentViewer";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { readDocument: vi.fn() },
}));

// pdf.js needs a real canvas; this stub reports pages and lets tests "draw".
vi.mock("./PdfCanvas", () => ({
  default: ({ page, onPageCount, highlights, drawing, onRegion }: { page: number; onPageCount: (count: number) => void; highlights: Region[]; drawing: boolean; onRegion: (region: Region) => void }) => {
    useEffect(() => onPageCount(3), [onPageCount]);
    return (
      <div>
        <p>Rendered page {page}</p>
        <p data-testid="highlights">{highlights.length}</p>
        {drawing ? <button type="button" onClick={() => onRegion({ x: 0.1, y: 0.2, width: 0.3, height: 0.1 })}>draw</button> : null}
      </div>
    );
  },
}));

function renderViewer(overrides: Partial<Parameters<typeof DocumentViewer>[0]> = {}) {
  const onPage = vi.fn();
  const onRegion = vi.fn();
  const view = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DocumentViewer courseId="course_1" resourceId="pdf_1" title="Handout" page={1} onPage={onPage} highlights={[]} pendingRegion={null} onRegion={onRegion} {...overrides} />
    </QueryClientProvider>,
  );
  return { view, onPage, onRegion };
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("document viewer", () => {
  it("loads verified bytes and pages through the document", async () => {
    vi.mocked(libraryApi.readDocument).mockResolvedValue(new ArrayBuffer(8));
    const { view, onPage } = renderViewer({ page: 2, highlights: [{ x: 0, y: 0, width: 0.5, height: 0.5 }] });
    await waitFor(() => expect(view.container.textContent).toContain("Rendered page 2"));
    expect(libraryApi.readDocument).toHaveBeenCalledWith("course_1", "pdf_1");
    await waitFor(() => expect(view.container.textContent).toContain("Page 2 of 3"));
    expect(view.getByTestId("highlights").textContent).toBe("1");
    fireEvent.click(view.getByRole("button", { name: "Next page" }));
    expect(onPage).toHaveBeenCalledWith(3);
    fireEvent.click(view.getByRole("button", { name: "Previous page" }));
    expect(onPage).toHaveBeenCalledWith(1);
  });

  it("highlights an area and shows the pending highlight until it is cleared", async () => {
    vi.mocked(libraryApi.readDocument).mockResolvedValue(new ArrayBuffer(8));
    const { view, onRegion } = renderViewer();
    const toggle = await waitFor(() => view.getByRole("button", { name: /Highlight area/ }));
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "draw" })));
    expect(onRegion).toHaveBeenCalledWith({ x: 0.1, y: 0.2, width: 0.3, height: 0.1 });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    cleanup();

    const pending = renderViewer({ pendingRegion: { x: 0.1, y: 0.2, width: 0.3, height: 0.1 } });
    await waitFor(() => expect(pending.view.container.textContent).toContain("Area highlighted on page 1"));
    expect(pending.view.getByTestId("highlights").textContent).toBe("1");
    fireEvent.click(pending.view.getByRole("button", { name: "clear it" }));
    expect(pending.onRegion).toHaveBeenCalledWith(null);
  });

  it("explains documents that cannot be opened", async () => {
    vi.mocked(libraryApi.readDocument).mockRejectedValue({ code: "document_changed", message: "the document on disk no longer matches the course" });
    const { view } = renderViewer();
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("no longer matches the course"));
  });
});
