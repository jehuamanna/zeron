import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { publisherApi } from "../services/publisher";
import { App } from "./App";

vi.mock("../services/publisher", () => ({
  publisherApi: { listDrafts: vi.fn(), createDraft: vi.fn(), getDraft: vi.fn() },
}));

function renderApp() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><App /></QueryClientProvider>);
}

describe("publisher courses", () => {
  it("lists path-free draft summaries", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([{ draftId: "draft_one", courseId: "course_one", courseVersion: "1.0.0", title: "Systems Course", revision: 2, updatedAtUnixSeconds: 1 }]);
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Systems Course"));
    expect(view.container.textContent).not.toContain("/home/");
  });

  it("creates and opens a durable draft", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([]);
    const created = { draftId: "draft_new", courseId: "course_new", courseVersion: "0.1.0", title: "Untitled course", revision: 0, updatedAtUnixSeconds: 1, description: null, publisherName: "Local publisher", language: "en", modules: [] };
    vi.mocked(publisherApi.createDraft).mockResolvedValue(created);
    vi.mocked(publisherApi.getDraft).mockResolvedValue(created);
    const view = renderApp();
    const button = await waitFor(() => Array.from(view.container.querySelectorAll("button")).find((item) => item.textContent?.includes("New course"))!);
    fireEvent.click(button);
    await waitFor(() => expect(view.container.textContent).toContain("Course builder"));
    expect(publisherApi.createDraft).toHaveBeenCalledOnce();
  });
});
