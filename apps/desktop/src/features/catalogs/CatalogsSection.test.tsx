import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type Catalog } from "../../services/library";
import { useLibraryUi } from "../../stores/library";
import { CatalogsSection } from "./CatalogsSection";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { listCatalogs: vi.fn(), chooseCatalogFile: vi.fn(), importCatalogUrl: vi.fn(), refreshCatalog: vi.fn(), removeCatalog: vi.fn(), importMagnet: vi.fn() },
}));

const catalog: Catalog = {
  publisherId: `ed25519-${"a".repeat(64)}`,
  fingerprint: "7F3A-91C2-0B44-E8D1",
  title: "Ana's courses",
  issuedAtUnixSeconds: 1_760_000_000,
  sourceUrl: "https://courses.example/zeron-catalog.json",
  entries: [
    { courseId: "course_rust", courseVersion: "1.2.0", title: "Rust Systems", description: "Ownership and lifetimes", magnet: `magnet:?xt=urn:btih:${"b".repeat(40)}`, infoHash: "b".repeat(40), status: "update" },
    { courseId: "course_go", courseVersion: "0.1.0", title: "Go Basics", description: "", magnet: `magnet:?xt=urn:btih:${"c".repeat(40)}`, infoHash: "c".repeat(40), status: "in_library" },
    { courseId: "course_new", courseVersion: "1.0.0", title: "Distributed Caching", description: "", magnet: `magnet:?xt=urn:btih:${"d".repeat(40)}`, infoHash: "d".repeat(40), status: "available" },
  ],
};

function renderSection(available = true) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><CatalogsSection torrentsAvailable={available} /></QueryClientProvider>);
}

beforeEach(() => {
  vi.mocked(libraryApi.listCatalogs).mockResolvedValue([catalog]);
  useLibraryUi.setState({ torrentImport: null });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("catalogs", () => {
  it("lists a catalog with its fingerprint and entries by status", async () => {
    const view = renderSection();
    const card = await waitFor(() => view.getByRole("button", { name: /^Ana's courses/ }));
    expect(card.textContent).toContain("7F3A-91C2-0B44-E8D1");
    expect(card.textContent).toContain("3 courses · 1 update");
    fireEvent.click(card);
    const courses = view.getByRole("list", { name: "Ana's courses courses" });
    const rows = within(courses).getAllByRole("listitem");
    expect(rows[0]!.textContent).toContain("Update available");
    expect(within(rows[0]!).getByRole("button", { name: /Update/ })).toBeTruthy();
    expect(rows[1]!.textContent).toContain("In your library");
    expect(within(rows[1]!).queryByRole("button")).toBeNull();
    expect(within(rows[2]!).getByRole("button", { name: /Import/ })).toBeTruthy();
  });

  it("imports an entry through the magnet import and tracks it", async () => {
    vi.mocked(libraryApi.importMagnet).mockResolvedValue({ jobId: "import_1" } as never);
    const view = renderSection();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /^Ana's courses/ })));
    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    await waitFor(() => expect(libraryApi.importMagnet).toHaveBeenCalledWith(catalog.entries[2]!.magnet));
    expect(useLibraryUi.getState().torrentImport).toEqual({ jobId: "import_1", source: { kind: "magnet", uri: catalog.entries[2]!.magnet } });
  });

  it("follows an address, refreshes, forgets, and reports refusals", async () => {
    vi.mocked(libraryApi.importCatalogUrl).mockRejectedValueOnce({ code: "catalog_invalid", message: "the catalog signature does not verify" }).mockResolvedValueOnce(catalog);
    vi.mocked(libraryApi.refreshCatalog).mockResolvedValue(catalog);
    vi.mocked(libraryApi.removeCatalog).mockResolvedValue(true);
    const view = renderSection();
    await waitFor(() => view.getByRole("button", { name: /^Ana's courses/ }));
    fireEvent.click(view.getByRole("button", { name: /Follow address/ }));
    fireEvent.change(view.getByLabelText("Catalog address"), { target: { value: "https://courses.example/zeron-catalog.json" } });
    fireEvent.click(view.getByRole("button", { name: "Follow" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("does not verify"));
    fireEvent.click(view.getByRole("button", { name: "Follow" }));
    await waitFor(() => expect(libraryApi.importCatalogUrl).toHaveBeenCalledTimes(2));
    fireEvent.click(view.getByRole("button", { name: "Refresh Ana's courses" }));
    await waitFor(() => expect(libraryApi.refreshCatalog).toHaveBeenCalledWith(catalog.publisherId));
    fireEvent.click(view.getByRole("button", { name: "Forget Ana's courses" }));
    await waitFor(() => expect(libraryApi.removeCatalog).toHaveBeenCalledWith(catalog.publisherId));
  });

  it("cannot import while torrents are unavailable", async () => {
    const view = renderSection(false);
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /^Ana's courses/ })));
    expect((view.getByRole("button", { name: /Import/ }) as HTMLButtonElement).disabled).toBe(true);
  });
});
