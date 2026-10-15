import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type CourseSharing } from "../../services/library";
import { CourseSharingCard } from "./CourseSharingCard";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: {
    getCourseSharing: vi.fn(),
    setCourseSharing: vi.fn(),
  },
}));

const status = (overrides: Partial<CourseSharing>): CourseSharing => ({
  courseId: "course_torrent",
  available: true,
  sharing: false,
  networkProfile: "local_network",
  state: "inactive",
  connectedPeers: 0,
  uploadedBytes: 0,
  uploadBytesPerSecond: 0,
  ...overrides,
});

function renderCard() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={queryClient}>
      <CourseSharingCard courseId="course_torrent" />
    </QueryClientProvider>,
  );
  return { view, invalidate };
}

const toggle = (container: HTMLElement) => container.querySelector<HTMLButtonElement>('[role="switch"]');

describe("course sharing", () => {
  beforeEach(() => vi.clearAllMocks());

  it("is off by default and uploads nothing", async () => {
    vi.mocked(libraryApi.getCourseSharing).mockResolvedValue(status({}));
    const { view } = renderCard();

    await waitFor(() => expect(toggle(view.container)).not.toBeNull());
    expect(toggle(view.container)?.getAttribute("aria-checked")).toBe("false");
    expect(view.container.textContent).toContain("Off. Zeron uploads nothing for this course.");
    expect(view.container.textContent).not.toContain("Connections");
  });

  it("turns sharing on and shows metrics measured on this computer", async () => {
    vi.mocked(libraryApi.getCourseSharing).mockResolvedValue(status({}));
    vi.mocked(libraryApi.setCourseSharing).mockResolvedValue(
      status({ sharing: true, state: "ready", connectedPeers: 3, uploadedBytes: 2043, uploadBytesPerSecond: 500 }),
    );
    const { view, invalidate } = renderCard();

    await waitFor(() => expect(toggle(view.container)).not.toBeNull());
    fireEvent.click(toggle(view.container)!);
    await waitFor(() => expect(toggle(view.container)?.getAttribute("aria-checked")).toBe("true"));
    expect(libraryApi.setCourseSharing).toHaveBeenCalledWith("course_torrent", true);
    expect(view.container.textContent).toContain("other learners on your local network");
    expect(view.container.textContent).toContain("They can see this computer's IP address.");
    expect(view.container.textContent).toContain("Connections3");
    expect(view.container.textContent).toContain("2 kB");
    expect(view.container.textContent).toContain("This is not a count of downloads.");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["courses"] });
  });

  it("names the internet audience and reports a failed check", async () => {
    vi.mocked(libraryApi.getCourseSharing).mockResolvedValue(status({ sharing: true, networkProfile: "internet", state: "failed" }));
    const { view } = renderCard();

    await waitFor(() => expect(view.container.textContent).toContain("over the internet"));
    expect(view.container.textContent).toContain("nothing is being served");
  });

  it("explains when sharing is unavailable", async () => {
    vi.mocked(libraryApi.getCourseSharing).mockResolvedValue(status({ available: false }));
    const { view } = renderCard();

    await waitFor(() => expect(view.container.textContent).toContain("Sharing is unavailable"));
    expect(toggle(view.container)?.disabled).toBe(true);
  });

  it("shows why a choice could not be applied", async () => {
    vi.mocked(libraryApi.getCourseSharing).mockResolvedValue(status({}));
    vi.mocked(libraryApi.setCourseSharing).mockRejectedValue({
      code: "sharing_failed",
      message: "the course could not be moved between torrent engines; nothing changed",
    });
    const { view } = renderCard();

    await waitFor(() => expect(toggle(view.container)).not.toBeNull());
    fireEvent.click(toggle(view.container)!);
    await waitFor(() => expect(view.container.textContent).toContain("nothing changed"));
    expect(toggle(view.container)?.getAttribute("aria-checked")).toBe("false");
  });
});
