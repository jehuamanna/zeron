import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { publisherApi } from "../../services/publisher";
import { ReadinessPanel } from "./ReadinessPanel";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { validateDraft: vi.fn(), inspectMedia: vi.fn() },
}));

afterEach(cleanup);

function renderPanel(revision: number, client = new QueryClient({ defaultOptions: { queries: { retry: false } } })) {
  return {
    client,
    view: render(<QueryClientProvider client={client}><ReadinessPanel draftId="draft_one" revision={revision} /></QueryClientProvider>),
  };
}

describe("readiness panel", () => {
  it("checks media on demand and clears results when the draft changes", async () => {
    vi.mocked(publisherApi.validateDraft).mockResolvedValue([]);
    vi.mocked(publisherApi.inspectMedia).mockResolvedValue([
      { resourceId: "r1", field: "modules[0].lessons[0].resources[0]", fileName: "lecture.mp4", code: "mp4_not_fast_start", severity: "warning", message: "Re-export with fast start." },
      { resourceId: "r2", field: "modules[0].lessons[0].resources[1]", fileName: "captions.srt", code: "subtitle_format", severity: "advice", message: "Convert to WebVTT." },
    ]);
    const { client, view } = renderPanel(3);
    await waitFor(() => expect(view.container.textContent).toContain("No blocking draft issues"));
    expect(publisherApi.inspectMedia).not.toHaveBeenCalled();
    expect(view.container.textContent).toContain("Files are never changed.");

    fireEvent.click(view.getByRole("button", { name: /check media/i }));
    const list = await waitFor(() => view.getByRole("list", { name: "Media findings" }));
    expect(publisherApi.inspectMedia).toHaveBeenCalledWith("draft_one");
    expect(list.textContent).toContain("lecture.mp4");
    expect(list.textContent).toContain("Re-export with fast start.");
    expect(view.getByLabelText("Warning")).toBeTruthy();
    expect(view.getByLabelText("Advice")).toBeTruthy();
    expect(view.getByRole("button", { name: /check again/i })).toBeTruthy();

    view.rerender(<QueryClientProvider client={client}><ReadinessPanel draftId="draft_one" revision={4} /></QueryClientProvider>);
    expect(view.queryByRole("list", { name: "Media findings" })).toBeNull();
    expect(view.getByRole("button", { name: /check media/i })).toBeTruthy();
  });

  it("reports a clean media check", async () => {
    vi.mocked(publisherApi.validateDraft).mockResolvedValue([{ code: "empty_course", field: "modules", message: "add at least one module" }]);
    vi.mocked(publisherApi.inspectMedia).mockResolvedValue([]);
    const { view } = renderPanel(1);
    await waitFor(() => expect(view.getByRole("list", { name: "Blocking issues" }).textContent).toContain("add at least one module"));
    fireEvent.click(view.getByRole("button", { name: /check media/i }));
    await waitFor(() => expect(view.container.textContent).toContain("No media issues found."));
  });
});
