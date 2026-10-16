import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type CoursePublisher } from "../../services/library";
import { PublisherBadge } from "./PublisherBadge";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { forgetCoursePublisher: vi.fn() },
}));

const verified: CoursePublisher = { status: "verified", fingerprint: "7F3A-91C2-0B44-E8D1", publisherId: `ed25519-${"a".repeat(64)}`, signedAtUnixSeconds: 1 };

function renderBadge(publisher: CoursePublisher) {
  return render(<QueryClientProvider client={new QueryClient()}><PublisherBadge courseId="course_rust" publisher={publisher} /></QueryClientProvider>);
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("publisher badge", () => {
  it("shows a verified publisher's fingerprint and forgets it only after confirmation", async () => {
    vi.mocked(libraryApi.forgetCoursePublisher).mockResolvedValue({ ...verified, status: "signed" });
    const view = renderBadge(verified);
    expect(view.container.textContent).toContain("Verified publisher");
    expect(view.container.textContent).toContain("7F3A-91C2-0B44-E8D1");
    fireEvent.click(view.getByRole("button", { name: /Forget publisher/ }));
    const dialog = view.getByRole("alertdialog", { name: "Forget this publisher?" });
    expect(dialog.textContent).toContain("whoever signed it");
    expect(libraryApi.forgetCoursePublisher).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Forget publisher" }));
    await waitFor(() => expect(libraryApi.forgetCoursePublisher).toHaveBeenCalledWith("course_rust"));
  });

  it("warns that an unsigned course could be updated by anyone", () => {
    const view = renderBadge({ status: "unsigned", fingerprint: null, publisherId: null, signedAtUnixSeconds: null });
    expect(view.container.textContent).toContain("Unsigned course");
    expect(view.container.textContent).toContain("Anyone could publish an update");
    expect(view.queryByRole("button", { name: /Forget publisher/ })).toBeNull();
  });
});
