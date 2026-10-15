import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type TorrentImport } from "../../services/library";
import { useLibraryUi } from "../../stores/library";
import { TorrentImportPanel } from "./TorrentImportPanel";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: {
    chooseTorrentFile: vi.fn(),
    importMagnet: vi.fn(),
    getTorrentImport: vi.fn(),
    cancelTorrentImport: vi.fn(),
    onTorrentImport: vi.fn(),
  },
}));

const magnet = `magnet:?xt=urn:btih:${"ab".repeat(20)}`;

const snapshot = (overrides: Partial<TorrentImport>): TorrentImport => ({
  jobId: "import_one",
  sequence: 0,
  stage: "queued",
  infoHash: null,
  manifestVerifiedBytes: null,
  manifestTotalBytes: null,
  courseId: null,
  title: null,
  failure: null,
  ...overrides,
});

let emit: (job: TorrentImport) => void = () => {};

function renderPanel(engine: "ready" | "unavailable" | "pending" = "ready") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={queryClient}>
      <TorrentImportPanel engine={engine} />
    </QueryClientProvider>,
  );
  return { view, invalidate };
}

const button = (container: HTMLElement, label: string) =>
  Array.from(container.querySelectorAll("button")).find((candidate) => candidate.textContent?.includes(label));

async function pasteMagnet(container: HTMLElement) {
  fireEvent.click(button(container, "Paste magnet link")!);
  fireEvent.change(container.querySelector("#magnet-link")!, { target: { value: `  ${magnet}  ` } });
  fireEvent.submit(container.querySelector("form")!);
  await waitFor(() => expect(libraryApi.importMagnet).toHaveBeenCalledWith(magnet));
}

afterEach(cleanup);

describe("torrent import", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useLibraryUi.getState().dismissTorrentImport();
    vi.mocked(libraryApi.onTorrentImport).mockImplementation((handler) => {
      emit = handler;
      return Promise.resolve(() => {});
    });
  });

  it("follows a magnet import through its stages and refreshes the library", async () => {
    vi.mocked(libraryApi.importMagnet).mockResolvedValue(snapshot({}));
    vi.mocked(libraryApi.getTorrentImport).mockResolvedValue(snapshot({ sequence: 1, stage: "resolving_metadata" }));
    const { view, invalidate } = renderPanel();

    await pasteMagnet(view.container);
    await waitFor(() => expect(view.container.textContent).toContain("Importing from torrent"));
    await waitFor(() => expect(libraryApi.onTorrentImport).toHaveBeenCalled());
    act(() =>
      emit(snapshot({ sequence: 2, stage: "fetching_manifest", infoHash: "ab".repeat(20), manifestVerifiedBytes: 1000, manifestTotalBytes: 4000 })),
    );
    await waitFor(() => expect(view.container.textContent).toContain("course.json: 1 kB of 4 kB verified"));
    expect(view.container.querySelector('[aria-current="step"]')?.textContent).toContain("Download course.json");

    act(() => emit(snapshot({ sequence: 1, stage: "resolving_metadata" })));
    expect(view.container.querySelector('[aria-current="step"]')?.textContent).toContain("Download course.json");

    act(() => emit(snapshot({ sequence: 4, stage: "imported", courseId: "course_one", title: "Distributed Systems" })));
    await waitFor(() => expect(view.container.textContent).toContain("Added Distributed Systems to your library"));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["courses"] });
    expect(button(view.container, "Cancel")).toBeUndefined();
  });

  it("cancels a running import", async () => {
    vi.mocked(libraryApi.chooseTorrentFile).mockResolvedValue(snapshot({ stage: "resolving_metadata" }));
    vi.mocked(libraryApi.getTorrentImport).mockResolvedValue(snapshot({ stage: "resolving_metadata" }));
    vi.mocked(libraryApi.cancelTorrentImport).mockResolvedValue(true);
    const { view } = renderPanel();

    fireEvent.click(button(view.container, "Open .torrent file")!);
    await waitFor(() => expect(button(view.container, "Cancel")).toBeDefined());
    expect(button(view.container, "Open .torrent file")?.disabled).toBe(true);
    fireEvent.click(button(view.container, "Cancel")!);
    await waitFor(() => expect(libraryApi.cancelTorrentImport).toHaveBeenCalledWith("import_one"));

    await waitFor(() => expect(libraryApi.onTorrentImport).toHaveBeenCalled());
    act(() => emit(snapshot({ sequence: 3, stage: "cancelled" })));
    await waitFor(() => expect(view.container.textContent).toContain("Import cancelled"));
    expect(view.container.textContent).toContain("Nothing was added to your library.");
    expect(button(view.container, "Choose file again")).toBeDefined();
  });

  it("explains a failure and retries the same magnet link", async () => {
    vi.mocked(libraryApi.importMagnet).mockResolvedValue(snapshot({}));
    vi.mocked(libraryApi.getTorrentImport).mockResolvedValue(
      snapshot({
        sequence: 5,
        stage: "failed",
        failure: { code: "metadata_timeout", message: "No peer sent the torrent metadata in time." },
      }),
    );
    const { view } = renderPanel();

    await pasteMagnet(view.container);
    await waitFor(() => expect(view.container.textContent).toContain("No peer sent the torrent metadata in time."));
    vi.mocked(libraryApi.importMagnet).mockResolvedValue(snapshot({ jobId: "import_two" }));
    vi.mocked(libraryApi.getTorrentImport).mockResolvedValue(snapshot({ jobId: "import_two", stage: "resolving_metadata" }));
    fireEvent.click(button(view.container, "Retry")!);
    await waitFor(() => expect(libraryApi.importMagnet).toHaveBeenCalledTimes(2));
    expect(libraryApi.importMagnet).toHaveBeenLastCalledWith(magnet);
    await waitFor(() => expect(libraryApi.getTorrentImport).toHaveBeenCalledWith("import_two"));
  });

  it("shows why an import could not start", async () => {
    vi.mocked(libraryApi.importMagnet).mockRejectedValue({ code: "torrent_import_busy", message: "another course import is already running" });
    const { view } = renderPanel();

    await pasteMagnet(view.container);
    await waitFor(() => expect(view.container.textContent).toContain("another course import is already running"));
  });

  it("disables torrent actions when the engine is unavailable", () => {
    const { view } = renderPanel("unavailable");

    expect(view.container.textContent).toContain("torrent engine could not start");
    expect(button(view.container, "Open .torrent file")?.disabled).toBe(true);
    expect(button(view.container, "Paste magnet link")?.disabled).toBe(true);
  });

  it("waits for readiness without claiming the engine failed", () => {
    const { view } = renderPanel("pending");

    expect(view.container.textContent).not.toContain("could not start");
    expect(button(view.container, "Open .torrent file")?.disabled).toBe(true);
  });
});
