import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type DraftDetail } from "../services/publisher";
import { useBuilderUi } from "../stores/builder";
import { App } from "./App";

vi.mock("../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/publisher")>()),
  publisherApi: { listDrafts: vi.fn(), createDraft: vi.fn(), getDraft: vi.fn(), updateDraftMetadata: vi.fn(), validateDraft: vi.fn(), listReleases: vi.fn(), getDistribution: vi.fn(), onDrag: vi.fn(async () => () => undefined), onDrop: vi.fn(async () => () => undefined) },
}));

const created: DraftDetail = {
  draftId: "draft_new",
  courseId: "course_new",
  courseVersion: "0.1.0",
  title: "Untitled course",
  revision: 0,
  updatedAtUnixSeconds: 1,
  description: null,
  publisherName: "Local publisher",
  language: "en",
  category: null,
  tags: [],
  prerequisites: [],
  licenseName: null,
  modules: [],
};

function renderApp() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><App /></QueryClientProvider>);
}

beforeEach(() => {
  useBuilderUi.setState({ selectedDraftId: null, workspace: "courses" });
  vi.mocked(publisherApi.listReleases).mockResolvedValue([]);
  vi.mocked(publisherApi.getDistribution).mockResolvedValue({ configuredProfile: "local_network", activeProfile: "local_network", listenPort: null, peerPort: null, portForwarding: false, activePortForwarding: false, restartRequired: false, trackers: [], trackerLists: [], effectiveTrackerCount: 0, uploadLimitKib: null, engineError: null, releases: [] });
});
afterEach(cleanup);

describe("publisher courses", () => {
  it("switches between the courses and distribution workspaces", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([]);
    const view = renderApp();
    fireEvent.click(view.getByRole("button", { name: "Distribution" }));
    await waitFor(() => expect(view.container.textContent).toContain("Publish a release to seed it."));
    expect(view.getByRole("heading", { level: 1 }).textContent).toBe("Distribution");
    expect(view.queryByRole("button", { name: /New course/ })).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "Courses" }));
    await waitFor(() => expect(view.getByRole("heading", { level: 1 }).textContent).toBe("Publisher courses"));
  });

  it("lists path-free draft summaries", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([{ draftId: "draft_one", courseId: "course_one", courseVersion: "1.0.0", title: "Systems Course", revision: 2, updatedAtUnixSeconds: 1 }]);
    const view = renderApp();
    await waitFor(() => expect(view.container.textContent).toContain("Systems Course"));
    expect(view.container.textContent).not.toContain("/home/");
    vi.mocked(publisherApi.getDraft).mockResolvedValue(null);
    fireEvent.click(view.getByRole("button", { name: /Edit draft/ }));
    expect(useBuilderUi.getState().selectedDraftId).toBe("draft_one");
  });

  it("creates and opens a durable draft with readiness issues", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([]);
    vi.mocked(publisherApi.createDraft).mockResolvedValue(created);
    vi.mocked(publisherApi.getDraft).mockResolvedValue(created);
    vi.mocked(publisherApi.validateDraft).mockResolvedValue([{ code: "empty_course", field: "modules", message: "add at least one module" }]);
    const view = renderApp();
    const button = await waitFor(() => Array.from(view.container.querySelectorAll("button")).find((item) => item.textContent?.includes("New course"))!);
    fireEvent.click(button);
    await waitFor(() => expect(view.container.textContent).toContain("Course builder"));
    expect(publisherApi.createDraft).toHaveBeenCalledOnce();
    await waitFor(() => expect(view.getByLabelText("Blocking issues").textContent).toContain("add at least one module"));
    expect((view.getByLabelText("Title") as HTMLInputElement).value).toBe("Untitled course");
    expect(view.getByRole("status").textContent).toBe(" Saved");
    expect(view.container.textContent).toContain("Draft revision r0");
  });
});
