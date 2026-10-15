import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { publisherApi } from "../../services/publisher";
import { ReleaseList } from "./ReleaseList";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { listReleases: vi.fn() },
}));

afterEach(cleanup);

function renderList() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><ReleaseList /></QueryClientProvider>);
}

describe("release history", () => {
  it("lists releases with status and short info hash", async () => {
    vi.mocked(publisherApi.listReleases).mockResolvedValue([
      { releaseId: "release_2", courseId: "c", courseVersion: "1.1.0", title: "Rust Systems", infoHash: "0123456789abcdef0123456789abcdef01234567", magnet: "m", pieceLength: 16384, pieceCount: 2, fileCount: 3, totalBytes: 2048, publishedAtUnixSeconds: 1_760_000_000, status: "published" },
      { releaseId: "release_1", courseId: "c", courseVersion: "1.0.0", title: "Rust Systems", infoHash: "fedcba9876543210fedcba9876543210fedcba98", magnet: "m", pieceLength: 16384, pieceCount: 1, fileCount: 2, totalBytes: 1024, publishedAtUnixSeconds: 1_750_000_000, status: "superseded" },
    ]);
    const view = renderList();
    const list = await waitFor(() => view.getByRole("list", { name: "Release history" }));
    expect(list.children).toHaveLength(2);
    expect(list.textContent).toContain("Version 1.1.0");
    expect(list.textContent).toContain("0123456789ab…");
    expect(list.textContent).toContain("published");
    expect(list.textContent).toContain("superseded");
    expect(list.textContent).toContain("2.0 KB");
  });

  it("explains the empty state", async () => {
    vi.mocked(publisherApi.listReleases).mockResolvedValue([]);
    const view = renderList();
    await waitFor(() => expect(view.container.textContent).toContain("No releases yet."));
  });
});
