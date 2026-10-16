import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type ActivityEvent } from "../../services/publisher";
import { ActivityLog } from "./ActivityLog";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { getDistributionActivity: vi.fn(), exportDistributionActivity: vi.fn() },
}));

function event(overrides: Partial<ActivityEvent>): ActivityEvent {
  return { eventId: 1, atUnixSeconds: 1_760_000_000, releaseId: "release_2", courseVersion: "1.1.0", kind: "seeding_started", detail: "Seeding version 1.1.0", ...overrides };
}

function renderLog(releaseId: string | null = null) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><ActivityLog releaseId={releaseId} /></QueryClientProvider>);
}

beforeEach(() => {
  vi.mocked(publisherApi.getDistributionActivity).mockResolvedValue([
    event({ eventId: 3, kind: "seeding_failed", detail: "Seeding could not start: cannot bind <address>" }),
    event({ eventId: 2, releaseId: null, courseVersion: null, kind: "tracker_lists_refreshed", detail: "Downloaded 2 tracker lists with 70 trackers" }),
    event({ eventId: 1, kind: "release_published", detail: "Version 1.1.0 published" }),
  ]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("distribution activity", () => {
  it("lists every event newest first with versions and redacted details", async () => {
    const view = renderLog();
    const list = await waitFor(() => view.getByRole("list", { name: "Activity" }));
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      expect.stringContaining("Seeding failed · v1.1.0Seeding could not start: cannot bind <address>"),
      expect.stringContaining("Tracker listsDownloaded 2 tracker lists with 70 trackers"),
      expect.stringContaining("Published · v1.1.0Version 1.1.0 published"),
    ]);
    expect(publisherApi.getDistributionActivity).toHaveBeenCalledWith(null);
    expect(view.container.textContent).toContain("File paths, network addresses, and private parts of links are removed");
  });

  it("shows one release's recent events without the export", async () => {
    const view = renderLog("release_2");
    await waitFor(() => expect(view.getByRole("list", { name: "Recent activity" })).toBeTruthy());
    expect(publisherApi.getDistributionActivity).toHaveBeenCalledWith("release_2");
    expect(view.queryByRole("button", { name: /Export log/ })).toBeNull();
    expect(view.container.textContent).not.toContain("· v1.1.0");
  });

  it("exports the log and reports cancel, success, and failure", async () => {
    vi.mocked(publisherApi.exportDistributionActivity).mockResolvedValueOnce(false).mockResolvedValueOnce(true).mockRejectedValueOnce({ code: "export_failed", message: "the activity log could not be written to the chosen location" });
    const view = renderLog();
    const button = await waitFor(() => view.getByRole("button", { name: /Export log/ }));
    fireEvent.click(button);
    await waitFor(() => expect(publisherApi.exportDistributionActivity).toHaveBeenCalledTimes(1));
    expect(view.queryByRole("status")).toBeNull();
    fireEvent.click(button);
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Activity log saved."));
    fireEvent.click(button);
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("could not be written"));
  });

  it("says when nothing has happened yet", async () => {
    vi.mocked(publisherApi.getDistributionActivity).mockResolvedValue([]);
    const view = renderLog();
    await waitFor(() => expect(view.container.textContent).toContain("Nothing has happened yet."));
  });
});
