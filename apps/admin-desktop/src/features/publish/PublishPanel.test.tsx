import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type PublishJob } from "../../services/publisher";
import { useBuilderUi } from "../../stores/builder";
import { PublishPanel } from "./PublishPanel";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: {
    validateDraft: vi.fn(),
    startPublishPreparation: vi.fn(),
    getPublishJob: vi.fn(),
    cancelPublishJob: vi.fn(),
    onPublishJob: vi.fn(),
    activateRelease: vi.fn(),
  },
}));

let server: PublishJob | null = null;
let emit: (job: PublishJob) => void = () => {};

function job(overrides: Partial<PublishJob>): PublishJob {
  return { jobId: "job_one", draftId: "draft_one", draftRevision: 4, sequence: 1, stage: "VALIDATING", progress: null, prepared: null, failure: null, releaseId: null, ...overrides };
}

function push(next: PublishJob) {
  server = next;
  act(() => emit(next));
}

beforeEach(() => {
  server = null;
  useBuilderUi.setState({ publishJobIds: {} });
  vi.mocked(publisherApi.validateDraft).mockResolvedValue([]);
  vi.mocked(publisherApi.getPublishJob).mockImplementation(async () => server);
  vi.mocked(publisherApi.startPublishPreparation).mockImplementation(async () => {
    server = job({});
    return server;
  });
  vi.mocked(publisherApi.cancelPublishJob).mockResolvedValue(true);
  vi.mocked(publisherApi.onPublishJob).mockImplementation(async (handler) => {
    emit = handler;
    return () => {
      emit = () => {};
    };
  });
});
afterEach(cleanup);

function renderPanel(revision = 4) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PublishPanel draftId="draft_one" revision={revision} />
    </QueryClientProvider>,
  );
}

function readyPrepareButton(view: ReturnType<typeof renderPanel>) {
  return waitFor(() => {
    const button = view.getByRole("button", { name: /prepare release/i }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    return button;
  });
}

describe("release preparation", () => {
  it("follows job events, ignores stale ones, and shows the verified result", async () => {
    const view = renderPanel();
    fireEvent.click(await readyPrepareButton(view));
    await waitFor(() => expect(view.getByRole("list", { name: "Preparation steps" })).toBeTruthy());
    await waitFor(() => expect(publisherApi.onPublishJob).toHaveBeenCalled());

    push(job({ sequence: 3, stage: "PACKAGING", progress: { completedBytes: 50, totalBytes: 100 } }));
    await waitFor(() => expect(view.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("50"));
    expect(view.container.querySelector("[aria-current=step]")?.textContent).toContain("Copy files");
    act(() => emit(job({ sequence: 2, stage: "VALIDATING" })));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(view.container.querySelector("[aria-current=step]")?.textContent).toContain("Copy files");
    expect(view.getByRole("button", { name: "Cancel" })).toBeTruthy();

    push(
      job({
        sequence: 9,
        stage: "READY_TO_PUBLISH",
        prepared: {
          attemptId: "stage_x",
          courseVersion: "1.0.0",
          resourceCount: 1,
          manifestSha256: "f".repeat(64),
          infoHash: "a".repeat(40),
          magnet: `magnet:?xt=urn:btih:${"a".repeat(40)}&dn=course-1.0.0`,
          trackerCount: 0,
          pieceLength: 16384,
          pieceCount: 3,
          fileCount: 2,
          totalBytes: 40_000,
        },
      }),
    );
    await waitFor(() => expect(view.container.textContent).toContain("Verified and ready to publish."));
    expect(view.container.textContent).toContain("a".repeat(40));
    expect(view.container.textContent).toContain("None: learners find it through local discovery or the DHT");
    expect((view.getByLabelText("Magnet link") as HTMLInputElement).value).toContain("urn:btih:");
    expect(view.container.textContent).toContain("3 × 16 KB");
    expect(view.getByRole("button", { name: /prepare again/i })).toBeTruthy();
    expect(view.queryByRole("progressbar")).toBeNull();
  });

  it("is blocked while the draft has validation issues", async () => {
    vi.mocked(publisherApi.validateDraft).mockResolvedValue([{ code: "empty_course", field: "modules", message: "add at least one module" }]);
    const view = renderPanel();
    await waitFor(() => expect(view.container.textContent).toContain("Resolve the blocking issues above first."));
    expect((view.getByRole("button", { name: /prepare release/i }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("cancels a running job and reports failures", async () => {
    const view = renderPanel();
    fireEvent.click(await readyPrepareButton(view));
    const cancel = await waitFor(() => view.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(publisherApi.onPublishJob).toHaveBeenCalled());
    fireEvent.click(cancel);
    await waitFor(() => expect(publisherApi.cancelPublishJob).toHaveBeenCalledWith("job_one"));
    push(job({ sequence: 5, stage: "CANCELLED" }));
    await waitFor(() => expect(view.container.textContent).toContain("Partial files were removed."));

    push(job({ sequence: 6, stage: "FAILED", failure: { code: "source_changed", message: "the source for resource `r1` changed while it was copied" } }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("changed while it was copied"));
  });

  it("publishes only after explicit confirmation and reports reused versions", async () => {
    const prepared = { attemptId: "stage_x", courseVersion: "1.0.0", resourceCount: 1, manifestSha256: "f".repeat(64), infoHash: "c".repeat(40), magnet: "magnet:?xt=urn:btih:c", trackerCount: 2, pieceLength: 16384, pieceCount: 1, fileCount: 2, totalBytes: 10 };
    server = job({ sequence: 4, stage: "READY_TO_PUBLISH", prepared });
    useBuilderUi.setState({ publishJobIds: { draft_one: "job_one" } });
    vi.mocked(publisherApi.activateRelease).mockRejectedValueOnce({ code: "version_already_released", message: "version 1.0.0 was already released; choose a new version" });
    vi.mocked(publisherApi.activateRelease).mockImplementationOnce(async () => {
      server = job({ sequence: 5, stage: "PUBLISHED", prepared, releaseId: "release_one" });
      return { releaseId: "release_one", courseId: "course_one", courseVersion: "1.0.0", title: "Course", infoHash: prepared.infoHash, magnet: prepared.magnet, pieceLength: 16384, pieceCount: 1, fileCount: 2, totalBytes: 10, publishedAtUnixSeconds: 1, status: "published", files: "present" };
    });
    const view = renderPanel();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /publish release/i })));
    const confirm = view.getByRole("group", { name: "Confirm publishing" });
    expect(confirm.textContent).toContain("cannot be reused");
    expect(document.activeElement?.textContent).toBe("Cancel");
    expect(publisherApi.activateRelease).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: "Publish permanently" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("choose a new version"));
    fireEvent.click(view.getByRole("button", { name: "Publish permanently" }));
    await waitFor(() => expect(view.container.textContent).toContain("Published version 1.0.0. Its files can no longer change."));
    expect(publisherApi.activateRelease).toHaveBeenCalledWith("job_one");
    expect(view.queryByRole("button", { name: /publish release/i })).toBeNull();
  });

  it("flags results prepared from an older draft revision", async () => {
    server = job({ sequence: 4, stage: "READY_TO_PUBLISH", draftRevision: 2, prepared: { attemptId: "stage_x", courseVersion: "1.0.0", resourceCount: 1, manifestSha256: "f".repeat(64), infoHash: "b".repeat(40), magnet: "magnet:?xt=urn:btih:b", trackerCount: 2, pieceLength: 16384, pieceCount: 1, fileCount: 2, totalBytes: 10 } });
    useBuilderUi.setState({ publishJobIds: { draft_one: "job_one" } });
    const view = renderPanel(5);
    await waitFor(() => expect(view.container.textContent).toContain("Prepared from r2; the draft has changed since."));
    expect(view.container.textContent).toMatch(/Trackers\s*2/);
  });
});
