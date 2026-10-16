import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BackupSettings } from "../features/network/BackupSettings";
import { CatalogsSection } from "../features/catalogs/CatalogsSection";
import { AnnotationMovesCard } from "../features/migration/AnnotationMovesCard";
import { PublisherBadge } from "../features/publisher/PublisherBadge";
import { CatalogPage } from "../publish/features/catalog/CatalogPage";
import { ActivityLog } from "../publish/features/distribution/ActivityLog";
import { ConfigurationSection } from "../publish/features/distribution/ConfigurationSection";
import { IdentityPage } from "../publish/features/identity/IdentityPage";
import { libraryApi, type CourseDetail } from "../services/library";
import { publisherApi } from "../publish/services/publisher";
import { accessibilityViolations } from "./accessibility";

vi.mock("../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/library")>()),
  libraryApi: {
    pendingRestore: vi.fn(async () => ({ createdAtUnixSeconds: 1, contents: ["library"], totalBytes: 10 })),
    exportBackup: vi.fn(),
    chooseBackupToRestore: vi.fn(),
    cancelRestore: vi.fn(),
    restartApp: vi.fn(),
    listCatalogs: vi.fn(async () => [{
      publisherId: `ed25519-${"a".repeat(64)}`, fingerprint: "7F3A-91C2-0B44-E8D1", title: "Ana's courses", issuedAtUnixSeconds: 1, sourceUrl: "https://courses.example/c.json",
      entries: [{ courseId: "c", courseVersion: "1.0.0", title: "Rust", description: "About", magnet: `magnet:?xt=urn:btih:${"b".repeat(40)}`, infoHash: "b".repeat(40), status: "available" }],
    }]),
    chooseCatalogFile: vi.fn(), importCatalogUrl: vi.fn(), refreshCatalog: vi.fn(), removeCatalog: vi.fn(), importMagnet: vi.fn(),
    annotationMoves: vi.fn(async () => [{ fromLessonId: "l", fromTitle: "Intro", fromVersion: "1.0.0", suggestedLessonId: "n", suggestedTitle: "Introduction", reason: "same_content", notes: 2, notesWidened: 0, hasDrawing: false, hasProgress: false }]),
    moveAnnotations: vi.fn(),
    forgetCoursePublisher: vi.fn(),
  },
}));

vi.mock("../publish/services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../publish/services/publisher")>()),
  publisherApi: {
    getIdentity: vi.fn(async () => ({ status: "ready", publisherId: `ed25519-${"a".repeat(64)}`, signingKey: `ed25519-${"a".repeat(64)}`, fingerprint: "7F3A-91C2-0B44-E8D1", createdAtUnixSeconds: 1, rotations: 1, signReleases: true, problem: null })),
    createIdentity: vi.fn(), rotateIdentity: vi.fn(), setSignReleases: vi.fn(), exportIdentityBackup: vi.fn(), restoreIdentityBackup: vi.fn(),
    getCatalog: vi.fn(async () => ({
      title: "My courses",
      listings: [{ release: { releaseId: "r", courseId: "c", courseVersion: "1.0.0", title: "Rust", infoHash: "0".repeat(40), magnet: "magnet:?xt=urn:btih:0", pieceLength: 1, pieceCount: 1, fileCount: 1, totalBytes: 1, publishedAtUnixSeconds: 1, status: "published", files: "present" }, listed: true }],
      revocations: [],
      retiredKeys: [{ key: `ed25519-${"b".repeat(64)}`, fingerprint: "1111-2222-3333-4444", retiredAtUnixSeconds: 1 }],
    })),
    setCatalogTitle: vi.fn(), setCatalogRelease: vi.fn(), revokeRetiredKey: vi.fn(), exportCatalog: vi.fn(),
    getDistributionActivity: vi.fn(async () => [{ eventId: 1, atUnixSeconds: 1, releaseId: null, courseVersion: null, kind: "tracker_lists_refreshed", detail: "Downloaded 1 tracker list" }]),
    exportDistributionActivity: vi.fn(),
    exportPublisherConfiguration: vi.fn(), choosePublisherConfiguration: vi.fn(), applyPublisherConfiguration: vi.fn(), discardPublisherConfiguration: vi.fn(),
  },
}));

const course = {
  courseId: "course", courseVersion: "1.1.0", title: "Systems", description: null, publisherName: "P", source: "local", torrentInfoHash: null,
  publisher: { status: "verified", fingerprint: "7F3A-91C2-0B44-E8D1", publisherId: "x", signedAtUnixSeconds: 1 },
  modules: [{ id: "m", title: "Module", lessons: [{ id: "n", title: "Introduction", resources: [] }] }],
} as unknown as CourseDetail;

function renderWithQueries(node: ReactNode) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>{node}</QueryClientProvider>);
}

afterEach(() => {
  cleanup();
});

describe("accessibility", () => {
  it("Publish → Identity, Catalog, and Distribution sections have no violations", async () => {
    for (const screen of [<IdentityPage key="identity" />, <CatalogPage key="catalog" onOpenIdentity={() => {}} />, <ConfigurationSection key="configuration" />, <ActivityLog key="activity" />]) {
      const view = renderWithQueries(screen);
      await waitFor(() => expect(view.container.textContent).not.toMatch(/^Reading|Loading/));
      await new Promise((resolve) => setTimeout(resolve, 20));
      expect(await accessibilityViolations(view.container)).toEqual([]);
      cleanup();
    }
  });

  it("Library catalogs, backup, publisher badge, and annotation moves have no violations", async () => {
    const view = renderWithQueries(
      <>
        <CatalogsSection torrentsAvailable />
        <BackupSettings />
        <PublisherBadge courseId="course" publisher={course.publisher} />
        <AnnotationMovesCard course={course} />
      </>,
    );
    await waitFor(() => expect(view.getByRole("button", { name: /^Ana's courses/ })).toBeTruthy());
    fireEvent.click(view.getByRole("button", { name: /^Ana's courses/ }));
    await waitFor(() => expect(view.getByRole("list", { name: "Annotations to move" })).toBeTruthy());
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });

  it("libraryApi and publisherApi mocks were used", () => {
    expect(libraryApi.listCatalogs).toBeDefined();
    expect(publisherApi.getIdentity).toBeDefined();
  });
});

describe("the accessibility check itself", () => {
  it("reports a real violation", async () => {
    const view = render(<div><img src="x.png" /><button type="button" /></div>);
    const violations = await accessibilityViolations(view.container);
    expect(violations.some((violation) => violation.startsWith("image-alt"))).toBe(true);
    expect(violations.some((violation) => violation.startsWith("button-name"))).toBe(true);
  });
});
