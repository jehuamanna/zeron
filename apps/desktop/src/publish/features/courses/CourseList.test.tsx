import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type Distribution, type DraftSummary, type Release, type SeedingStatus } from "../../services/publisher";
import { CourseList, groupCourses } from "./CourseList";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { listDrafts: vi.fn(), listReleases: vi.fn(), getDistribution: vi.fn(), getReleaseDetails: vi.fn(), exportReleaseTorrent: vi.fn(), deleteDraft: vi.fn(), archiveRelease: vi.fn(), deleteReleaseFiles: vi.fn(), makeReleaseCurrent: vi.fn(), createDraftFromRelease: vi.fn() },
}));

const magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=rust-systems";

function release(overrides: Partial<Release>): Release {
  return { releaseId: "release_2", courseId: "course_rust", courseVersion: "1.1.0", title: "Rust Systems", infoHash: "0123456789abcdef0123456789abcdef01234567", magnet, pieceLength: 16384, pieceCount: 2, fileCount: 3, totalBytes: 2048, publishedAtUnixSeconds: 1_760_000_000, status: "published", files: "present", ...overrides };
}

function draft(overrides: Partial<DraftSummary>): DraftSummary {
  return { draftId: "draft_rust", courseId: "course_rust", courseVersion: "1.2.0", title: "Rust Systems", revision: 7, updatedAtUnixSeconds: 1_770_000_000, ...overrides };
}

function seeding(overrides: Partial<SeedingStatus>): SeedingStatus {
  return { releaseId: "release_2", courseVersion: "1.1.0", title: "Rust Systems", releaseStatus: "published", enabled: true, state: "seeding", verifiedBytes: 2048, totalBytes: 2048, uploadedBytes: 0, uploadBytesPerSecond: 0, connectedPeers: 2, failure: null, ...overrides };
}

function distribution(releases: SeedingStatus[]): Distribution {
  return { trackers: [], trackerLists: [], effectiveTrackerCount: 0, engineError: null, releases };
}

const releases = [
  release({}),
  release({ releaseId: "release_1", courseVersion: "1.0.0", infoHash: "fedcba9876543210fedcba9876543210fedcba98", publishedAtUnixSeconds: 1_750_000_000, status: "superseded" }),
  release({ releaseId: "release_go", courseId: "course_go", courseVersion: "0.1.0", title: "Go Basics", publishedAtUnixSeconds: 1_740_000_000 }),
];

function renderList(onOpenDraft = vi.fn()) {
  const view = render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><CourseList onOpenDraft={onOpenDraft} /></QueryClientProvider>);
  return { view, onOpenDraft };
}

beforeEach(() => {
  vi.mocked(publisherApi.listDrafts).mockResolvedValue([draft({}), draft({ draftId: "draft_new", courseId: "course_new", title: "Fresh Course", courseVersion: "0.1.0", revision: 0, updatedAtUnixSeconds: 1_780_000_000 })]);
  vi.mocked(publisherApi.listReleases).mockResolvedValue(releases);
  vi.mocked(publisherApi.getDistribution).mockResolvedValue(distribution([seeding({}), seeding({ releaseId: "release_1", enabled: false, state: null })]));
  vi.mocked(publisherApi.getReleaseDetails).mockImplementation(async (releaseId) => ({
    release: releases.find((item) => item.releaseId === releaseId)!,
    files: [{ path: "course.json", byteLength: 512 }, { path: "resources/res_1/lecture.mp4", byteLength: 1536 }],
  }));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("course grouping", () => {
  it("groups drafts and releases by course, most recently active first", () => {
    const groups = groupCourses([draft({}), draft({ draftId: "draft_new", courseId: "course_new", title: "Fresh", updatedAtUnixSeconds: 1_780_000_000 })], releases);
    expect(groups.map((group) => group.courseId)).toEqual(["course_new", "course_rust", "course_go"]);
    expect(groups[1]!.drafts.map((item) => item.draftId)).toEqual(["draft_rust"]);
    expect(groups[1]!.releases.map((item) => item.releaseId)).toEqual(["release_2", "release_1"]);
    expect(groups[2]).toMatchObject({ title: "Go Basics", drafts: [] });
  });
});

describe("course management", () => {
  it("shows one card per course with its drafts, releases, and seeding state", async () => {
    const { view, onOpenDraft } = renderList();
    const courses = await waitFor(() => view.getByRole("list", { name: "Courses" }));
    expect(Array.from(courses.children).map((item) => item.getAttribute("aria-label"))).toEqual(["Fresh Course", "Rust Systems", "Go Basics"]);
    const rust = view.getByRole("listitem", { name: "Rust Systems" });
    expect(rust.textContent).toContain("1 draft · 2 releases");
    const history = within(rust).getByRole("list", { name: "Release history" });
    expect(history.children).toHaveLength(2);
    await waitFor(() => expect(history.textContent).toContain("Seeding · 2 peers"));
    expect(history.textContent).toContain("Not seeding");
    expect(history.textContent).toContain("superseded");
    fireEvent.click(within(rust).getByRole("button", { name: /Edit draft/ }));
    expect(onOpenDraft).toHaveBeenCalledWith("draft_rust");
  });

  it("reveals the full magnet, info hash, and declared files of a release", async () => {
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    const toggle = within(rust).getByRole("button", { name: /Version 1.1.0/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect((within(rust).getByLabelText("Magnet link") as HTMLInputElement).value).toBe(magnet);
    expect(rust.textContent).toContain("0123456789abcdef0123456789abcdef01234567");
    const files = await waitFor(() => within(rust).getByRole("list", { name: "Release files" }));
    expect(files.textContent).toContain("resources/res_1/lecture.mp4");
    expect(files.textContent).toContain("1.5 KB");
    expect(publisherApi.getReleaseDetails).toHaveBeenCalledWith("release_2");
  });

  it("copies the magnet, and selects it for manual copy when the clipboard is refused", async () => {
    const writeText = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const execCommand = vi.fn().mockReturnValue(false);
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    fireEvent.click(within(rust).getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(within(rust).getByRole("button", { name: "Copied" })).toBeTruthy());
    expect(writeText).toHaveBeenCalledWith(magnet);
    fireEvent.click(within(rust).getByRole("button", { name: "Copied" }));
    await waitFor(() => expect(within(rust).getByRole("status").textContent).toContain("press Ctrl+C"));
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(document.activeElement).toBe(within(rust).getByLabelText("Magnet link"));
  });

  it("exports the torrent by release id and reports the outcome", async () => {
    vi.mocked(publisherApi.exportReleaseTorrent).mockResolvedValueOnce(false).mockResolvedValueOnce(true).mockRejectedValueOnce({ code: "export_failed", message: "the torrent file could not be written to the chosen location" });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    const exportButton = within(rust).getByRole("button", { name: /Export .torrent/ });
    fireEvent.click(exportButton);
    await waitFor(() => expect(publisherApi.exportReleaseTorrent).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(exportButton.hasAttribute("disabled")).toBe(false));
    expect(rust.textContent).not.toContain("Torrent file saved.");
    fireEvent.click(exportButton);
    await waitFor(() => expect(rust.textContent).toContain("Torrent file saved."));
    fireEvent.click(exportButton);
    await waitFor(() => expect(within(rust).getByRole("alert").textContent).toContain("could not be written"));
    expect(vi.mocked(publisherApi.exportReleaseTorrent).mock.calls).toEqual([["release_2"], ["release_2"], ["release_2"]]);
  });

  it("keeps releases visible when seeding state or details are unavailable", async () => {
    vi.mocked(publisherApi.getDistribution).mockRejectedValue({ code: "distribution_failed", message: "x" });
    vi.mocked(publisherApi.getReleaseDetails).mockRejectedValue({ code: "release_unavailable", message: "the released torrent differs from its record" });
    const { view } = renderList();
    const go = await waitFor(() => view.getByRole("listitem", { name: "Go Basics" }));
    await waitFor(() => expect(go.textContent).toContain("Seeding state unavailable"));
    fireEvent.click(within(go).getByRole("button", { name: /Version 0.1.0/ }));
    await waitFor(() => expect(within(go).getByRole("alert").textContent).toContain("differs from its record"));
    expect((within(go).getByLabelText("Magnet link") as HTMLInputElement).value).toBe(magnet);
  });

  it("explains the empty state", async () => {
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([]);
    vi.mocked(publisherApi.listReleases).mockResolvedValue([]);
    const { view } = renderList();
    await waitFor(() => expect(view.container.textContent).toContain("Create your first course"));
  });

  it("deletes a draft only after confirmation, and keeps its releases", async () => {
    vi.mocked(publisherApi.deleteDraft).mockResolvedValue(undefined);
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft v1.2.0" }));
    const dialog = within(rust).getByRole("alertdialog", { name: "Delete draft v1.2.0?" });
    expect(dialog.textContent).toContain("Published releases and your original media files are kept.");
    expect(document.activeElement?.textContent).toBe("Cancel");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(within(rust).queryByRole("alertdialog")).toBeNull();
    expect(publisherApi.deleteDraft).not.toHaveBeenCalled();

    vi.mocked(publisherApi.listDrafts).mockResolvedValue([draft({ draftId: "draft_new", courseId: "course_new", title: "Fresh Course", courseVersion: "0.1.0", revision: 0, updatedAtUnixSeconds: 1_780_000_000 })]);
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft v1.2.0" }));
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft" }));
    await waitFor(() => expect(publisherApi.deleteDraft).toHaveBeenCalledWith("draft_rust", 7));
    await waitFor(() => expect(view.getByRole("listitem", { name: "Rust Systems" }).textContent).toContain("0 drafts · 2 releases"));
    expect(within(view.getByRole("listitem", { name: "Rust Systems" })).queryByRole("button", { name: /Edit draft/ })).toBeNull();
  });

  it("explains why a draft could not be deleted", async () => {
    vi.mocked(publisherApi.deleteDraft)
      .mockRejectedValueOnce({ code: "draft_busy", message: "x" })
      .mockRejectedValueOnce({ code: "revision_conflict", message: "x", currentRevision: 8 });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft v1.2.0" }));
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft" }));
    await waitFor(() => expect(within(rust).getByRole("alert").textContent).toContain("Finish or cancel it first"));
    const listed = vi.mocked(publisherApi.listDrafts).mock.calls.length;
    fireEvent.click(within(rust).getByRole("button", { name: "Delete draft" }));
    await waitFor(() => expect(within(rust).getByRole("alert").textContent).toContain("saved since the list loaded"));
    await waitFor(() => expect(vi.mocked(publisherApi.listDrafts).mock.calls.length).toBeGreaterThan(listed));
  });

  it("archives a release after confirmation and shows it as archived", async () => {
    vi.mocked(publisherApi.archiveRelease).mockImplementation(async () => {
      const archived = release({ status: "archived" });
      vi.mocked(publisherApi.listReleases).mockResolvedValue([archived, ...releases.slice(1)]);
      vi.mocked(publisherApi.getDistribution).mockResolvedValue(distribution([seeding({ enabled: false, state: null, releaseStatus: "archived" })]));
      return archived;
    });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    fireEvent.click(within(rust).getByRole("button", { name: /Archive release/ }));
    const dialog = within(rust).getByRole("alertdialog", { name: "Archive version 1.1.0?" });
    expect(dialog.textContent).toContain("Seeding stops now");
    expect(dialog.textContent).toContain("release files stay on this computer");
    fireEvent.click(within(dialog).getByRole("button", { name: "Archive release" }));
    await waitFor(() => expect(publisherApi.archiveRelease).toHaveBeenCalledWith("release_2"));
    await waitFor(() => expect(rust.textContent).toContain("Archived. This release is no longer seeded"));
    expect(within(rust).queryByRole("button", { name: /Archive release/ })).toBeNull();
    const row = within(rust).getByRole("button", { name: /Version 1.1.0/ });
    expect(row.textContent).toContain("archived");
    expect(row.textContent).toContain("Not seeding");
  });

  it("keeps the release unarchived when archiving fails", async () => {
    vi.mocked(publisherApi.archiveRelease).mockRejectedValue({ code: "seeding_failed", message: "the engine could not stop the release" });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    fireEvent.click(within(rust).getByRole("button", { name: /Archive release/ }));
    fireEvent.click(within(rust).getByRole("button", { name: "Archive release" }));
    await waitFor(() => expect(within(rust).getByRole("alertdialog").textContent).toContain("could not stop the release"));
    expect(within(rust).getByRole("button", { name: /Version 1.1.0/ }).textContent).toContain("published");
  });

  it("deletes an archived release's files only after the version is typed", async () => {
    const archived = release({ status: "archived" });
    vi.mocked(publisherApi.listReleases).mockResolvedValue([archived, ...releases.slice(1)]);
    vi.mocked(publisherApi.deleteReleaseFiles)
      .mockRejectedValueOnce({ code: "release_delete_failed", message: "removing release files: permission denied" })
      .mockImplementationOnce(async () => {
        const deleted = release({ status: "archived", files: "deleted" });
        vi.mocked(publisherApi.listReleases).mockResolvedValue([deleted, ...releases.slice(1)]);
        return deleted;
      });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    expect(within(rust).queryByRole("button", { name: /Archive release/ })).toBeNull();
    fireEvent.click(within(rust).getByRole("button", { name: /Delete files/ }));
    const dialog = within(rust).getByRole("alertdialog", { name: "Delete the files of version 1.1.0?" });
    expect(dialog.textContent).toContain("cannot be undone");
    const confirm = within(dialog).getByRole("button", { name: "Delete files permanently" });
    const typed = within(dialog).getByLabelText("Type 1.1.0 to confirm");
    expect(confirm.hasAttribute("disabled")).toBe(true);
    fireEvent.change(typed, { target: { value: "1.1" } });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    fireEvent.change(typed, { target: { value: "1.1.0" } });
    expect(confirm.hasAttribute("disabled")).toBe(false);

    fireEvent.click(confirm);
    await waitFor(() => expect(within(dialog).getByRole("alert").textContent).toContain("permission denied"));
    fireEvent.click(confirm);
    await waitFor(() => expect(rust.textContent).toContain("its files were deleted"));
    expect(vi.mocked(publisherApi.deleteReleaseFiles).mock.calls).toEqual([["release_2", "1.1.0"], ["release_2", "1.1.0"]]);
    expect(within(rust).queryByRole("button", { name: /Export .torrent/ })).toBeNull();
    expect(within(rust).queryByRole("list", { name: "Release files" })).toBeNull();
    expect(within(rust).getByRole("button", { name: /Version 1.1.0/ }).textContent).toContain("Files deleted");
    expect((within(rust).getByLabelText("Magnet link") as HTMLInputElement).value).toBe(magnet);
  });

  it("offers a retry for an interrupted file deletion and never reads its torrent", async () => {
    vi.mocked(publisherApi.listReleases).mockResolvedValue([release({ status: "archived", files: "deleting" })]);
    vi.mocked(publisherApi.listDrafts).mockResolvedValue([]);
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    expect(rust.textContent).toContain("did not finish");
    expect(within(rust).getByRole("button", { name: /Delete files/ })).toBeTruthy();
    expect(publisherApi.getReleaseDetails).not.toHaveBeenCalled();
  });

  it("makes a superseded release current after confirmation", async () => {
    vi.mocked(publisherApi.makeReleaseCurrent).mockImplementation(async () => {
      const current = release({ releaseId: "release_1", courseVersion: "1.0.0", status: "published" });
      vi.mocked(publisherApi.listReleases).mockResolvedValue([release({ status: "superseded" }), current, releases[2]!]);
      return current;
    });
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.0.0/ }));
    expect(rust.textContent).toContain("Version 1.1.0 is the current release");
    fireEvent.click(within(rust).getByRole("button", { name: /Make current/ }));
    const dialog = within(rust).getByRole("alertdialog", { name: "Make version 1.0.0 current?" });
    expect(dialog.textContent).toContain("Version 1.1.0 becomes superseded");
    fireEvent.click(within(dialog).getByRole("button", { name: "Make current" }));
    await waitFor(() => expect(publisherApi.makeReleaseCurrent).toHaveBeenCalledWith("release_1"));
    await waitFor(() => expect(within(rust).getByRole("button", { name: /Version 1.0.0/ }).textContent).toContain("published"));
    expect(within(rust).getByRole("button", { name: /Version 1.1.0/ }).textContent).toContain("superseded");
  });

  it("offers no rollback for the current or an archived release", async () => {
    vi.mocked(publisherApi.listReleases).mockResolvedValue([release({}), release({ releaseId: "release_1", courseVersion: "1.0.0", status: "archived" })]);
    const { view } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    expect(rust.textContent).toContain("This is the course's current release");
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.0.0/ }));
    expect(within(rust).queryByRole("button", { name: /Make current/ })).toBeNull();
  });

  it("starts a new draft from a release and opens it", async () => {
    vi.mocked(publisherApi.createDraftFromRelease).mockResolvedValue({
      draftId: "draft_next", courseId: "course_rust", courseVersion: "1.1.1", title: "Rust Systems", revision: 0, updatedAtUnixSeconds: 1_790_000_000,
      description: null, publisherName: "P", language: "en", category: null, tags: [], prerequisites: [], licenseName: null, modules: [],
    });
    const { view, onOpenDraft } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    fireEvent.click(within(rust).getByRole("button", { name: /New draft from this version/ }));
    await waitFor(() => expect(onOpenDraft).toHaveBeenCalledWith("draft_next"));
    expect(publisherApi.createDraftFromRelease).toHaveBeenCalledWith("release_2");
  });

  it("explains when a draft cannot be started from a release", async () => {
    vi.mocked(publisherApi.createDraftFromRelease).mockRejectedValue({ code: "release_unavailable", message: "the released course.json differs from its record" });
    const { view, onOpenDraft } = renderList();
    const rust = await waitFor(() => view.getByRole("listitem", { name: "Rust Systems" }));
    fireEvent.click(within(rust).getByRole("button", { name: /Version 1.1.0/ }));
    fireEvent.click(within(rust).getByRole("button", { name: /New draft from this version/ }));
    await waitFor(() => expect(within(rust).getByRole("alert").textContent).toContain("differs from its record"));
    expect(onOpenDraft).not.toHaveBeenCalled();
  });
});
