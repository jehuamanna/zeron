import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type PublisherCatalog, type PublisherIdentity, type Release } from "../../services/publisher";
import { CatalogPage } from "./CatalogPage";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { getIdentity: vi.fn(), getCatalog: vi.fn(), setCatalogTitle: vi.fn(), setCatalogRelease: vi.fn(), revokeRetiredKey: vi.fn(), exportCatalog: vi.fn() },
}));

const ready: PublisherIdentity = { status: "ready", publisherId: `ed25519-${"a".repeat(64)}`, signingKey: `ed25519-${"a".repeat(64)}`, fingerprint: "7F3A-91C2-0B44-E8D1", createdAtUnixSeconds: 1, rotations: 1, signReleases: true, problem: null };
function release(overrides: Partial<Release>): Release {
  return { releaseId: "release_2", courseId: "course_rust", courseVersion: "1.1.0", title: "Rust Systems", infoHash: "0".repeat(40), magnet: "magnet:?xt=urn:btih:0", pieceLength: 16384, pieceCount: 1, fileCount: 2, totalBytes: 10, publishedAtUnixSeconds: 1, status: "published", files: "present", ...overrides };
}
const catalog: PublisherCatalog = {
  title: "My courses",
  listings: [{ release: release({}), listed: false }, { release: release({ releaseId: "release_1", courseVersion: "1.0.0", status: "archived" }), listed: false }],
  revocations: [],
  retiredKeys: [{ key: `ed25519-${"b".repeat(64)}`, fingerprint: "1111-2222-3333-4444", retiredAtUnixSeconds: 1_760_000_000 }],
};

function renderPage(onOpenIdentity = vi.fn()) {
  return { view: render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><CatalogPage onOpenIdentity={onOpenIdentity} /></QueryClientProvider>), onOpenIdentity };
}

beforeEach(() => {
  vi.mocked(publisherApi.getIdentity).mockResolvedValue(ready);
  vi.mocked(publisherApi.getCatalog).mockResolvedValue(catalog);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("publisher catalog", () => {
  it("asks for an identity first", async () => {
    vi.mocked(publisherApi.getIdentity).mockResolvedValue({ ...ready, status: "none" });
    const { view, onOpenIdentity } = renderPage();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /Create a signing identity/ })));
    expect(onOpenIdentity).toHaveBeenCalled();
  });

  it("lists a release only after explicit confirmation and never shows archived ones", async () => {
    vi.mocked(publisherApi.setCatalogRelease).mockResolvedValue({ ...catalog, listings: [{ release: release({}), listed: true }] });
    const { view } = renderPage();
    const list = await waitFor(() => view.getByRole("list", { name: "Catalog releases" }));
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    fireEvent.click(within(list).getByRole("button", { name: /List in catalog/ }));
    expect(publisherApi.setCatalogRelease).not.toHaveBeenCalled();
    const dialog = view.getByRole("alertdialog", { name: "List Rust Systems 1.1.0 in your catalog?" });
    expect(dialog.textContent).toContain("its magnet link");
    fireEvent.click(within(dialog).getByRole("button", { name: "List release" }));
    await waitFor(() => expect(publisherApi.setCatalogRelease).toHaveBeenCalledWith("release_2", true));
    await waitFor(() => expect(within(view.getByRole("list", { name: "Catalog releases" })).getByText("Listed")).toBeTruthy());
  });

  it("exports the signed catalog and saves its title", async () => {
    vi.mocked(publisherApi.exportCatalog).mockResolvedValue(true);
    vi.mocked(publisherApi.setCatalogTitle).mockResolvedValue({ ...catalog, title: "Ana's courses" });
    const { view } = renderPage();
    const title = await waitFor(() => view.getByLabelText("Title learners see"));
    fireEvent.change(title, { target: { value: "Ana's courses" } });
    fireEvent.click(view.getByRole("button", { name: "Save title" }));
    await waitFor(() => expect(publisherApi.setCatalogTitle).toHaveBeenCalledWith("Ana's courses"));
    fireEvent.click(view.getByRole("button", { name: /Export signed catalog/ }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Catalog saved."));
  });

  it("revokes a retired key with a reason", async () => {
    vi.mocked(publisherApi.revokeRetiredKey).mockResolvedValue({ ...catalog, retiredKeys: [], revocations: [{ revokedKey: catalog.retiredKeys[0]!.key, fingerprint: "1111-2222-3333-4444", revokedAtUnixSeconds: 1, reason: "laptop stolen" }] });
    const { view } = renderPage();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /Revoke/ })));
    const dialog = view.getByRole("alertdialog", { name: "Revoke this retired key?" });
    expect((within(dialog).getByRole("button", { name: "Revoke key" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(within(dialog).getByLabelText("Reason learners will see"), { target: { value: "laptop stolen" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke key" }));
    await waitFor(() => expect(publisherApi.revokeRetiredKey).toHaveBeenCalledWith(catalog.retiredKeys[0]!.key, "laptop stolen"));
    await waitFor(() => expect(view.container.textContent).toContain("revoked: laptop stolen"));
  });
});
