import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type DraftDetail } from "../../services/publisher";
import { createDraftWriter } from "./draftWriter";
import { MetadataForm } from "./MetadataForm";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { updateDraftMetadata: vi.fn() },
}));

const draft: DraftDetail = {
  draftId: "draft_one",
  courseId: "course_one",
  courseVersion: "0.1.0",
  title: "Untitled course",
  revision: 4,
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

afterEach(cleanup);

function renderForm(onSaved = vi.fn(), onConflict = vi.fn()) {
  const writer = createDraftWriter(draft.revision, { onSaved, onConflict });
  return render(<MetadataForm draft={draft} writer={writer} disabled={false} autosaveDelayMs={5} />);
}

describe("metadata autosave", () => {
  it("saves debounced edits against the owned revision", async () => {
    vi.mocked(publisherApi.updateDraftMetadata).mockImplementation(async (_id, revision, metadata) => ({ ...draft, ...metadata, revision: revision + 1 }));
    const onSaved = vi.fn();
    const view = renderForm(onSaved);
    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Systems" } });
    fireEvent.change(view.getByLabelText("Tags (comma separated)"), { target: { value: "rust, , systems" } });
    expect(view.getByRole("status").textContent).toContain("Unsaved changes");
    await waitFor(() => expect(view.getByRole("status").textContent).toBe(" Saved"));
    expect(publisherApi.updateDraftMetadata).toHaveBeenCalledOnce();
    expect(publisherApi.updateDraftMetadata).toHaveBeenCalledWith("draft_one", 4, expect.objectContaining({ title: "Systems", tags: ["rust", "systems"], description: null }));
    expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ revision: 5 }));

    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Systems II" } });
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ revision: 6 })));
    await waitFor(() => expect(view.getByRole("status").textContent).toBe(" Saved"));
    expect(publisherApi.updateDraftMetadata).toHaveBeenLastCalledWith("draft_one", 5, expect.objectContaining({ title: "Systems II" }));
  });

  it("stops autosave after a revision conflict", async () => {
    vi.mocked(publisherApi.updateDraftMetadata).mockRejectedValue({ code: "revision_conflict", message: "conflict", currentRevision: 7 });
    const onConflict = vi.fn();
    const view = renderForm(vi.fn(), onConflict);
    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Stale" } });
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("reload required"));
    expect(onConflict).toHaveBeenCalledWith(7);
    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Still stale" } });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(publisherApi.updateDraftMetadata).toHaveBeenCalledOnce();
  });

  it("reports rejected input and lets the editor retry", async () => {
    vi.mocked(publisherApi.updateDraftMetadata).mockRejectedValueOnce({ code: "invalid_input", message: "metadata.title is too long", field: "metadata.title" });
    vi.mocked(publisherApi.updateDraftMetadata).mockImplementationOnce(async (_id, revision, metadata) => ({ ...draft, ...metadata, revision: revision + 1 }));
    const view = renderForm();
    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Long" } });
    const alert = await waitFor(() => view.getByRole("alert"));
    expect(alert.textContent).toContain("too long");
    expect(view.getByLabelText("Title").getAttribute("aria-invalid")).toBe("true");
    fireEvent.click(view.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toBe(" Saved"));
  });
});
