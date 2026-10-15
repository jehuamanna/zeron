import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type NetworkSettings as Settings } from "../../services/library";
import { NetworkSettings } from "./NetworkSettings";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: {
    getNetworkSettings: vi.fn(),
    setNetworkProfile: vi.fn(),
  },
}));

const settings = (overrides: Partial<Settings>): Settings => ({
  configuredProfile: "local_network",
  activeProfile: "local_network",
  torrentEngine: "ready",
  downloadPort: 41213,
  sharingPort: 41099,
  ...overrides,
});

function renderSettings(onBack = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <NetworkSettings onBack={onBack} />
    </QueryClientProvider>,
  );
  return { view, onBack };
}

const radio = (container: HTMLElement, value: string) =>
  container.querySelector<HTMLInputElement>(`input[type="radio"][value="${value}"]`);

describe("network settings", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows the active local-network profile and this session's ports", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view } = renderSettings();

    await waitFor(() => expect(radio(view.container, "local_network")?.checked).toBe(true));
    expect(radio(view.container, "internet")?.checked).toBe(false);
    expect(view.container.textContent).toContain("downloads use port 41213");
    expect(view.container.textContent).toContain("shared courses are served on port 41099");
    expect(view.container.textContent).toContain("UPnP port mapping is never used.");
    expect(view.container.textContent).not.toContain("Restart Zeron");
  });

  it("saves the internet profile and asks for a restart", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValueOnce(settings({}));
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({ configuredProfile: "internet" }));
    vi.mocked(libraryApi.setNetworkProfile).mockResolvedValue();
    const { view } = renderSettings();

    await waitFor(() => expect(radio(view.container, "internet")).not.toBeNull());
    expect(view.container.textContent).toContain("Your IP address becomes visible to other DHT users");
    fireEvent.click(radio(view.container, "internet")!);
    await waitFor(() => expect(view.container.textContent).toContain("Restart Zeron to apply the new network setting."));
    expect(libraryApi.setNetworkProfile).toHaveBeenCalledWith("internet");
    expect(radio(view.container, "internet")?.checked).toBe(true);
  });

  it("explains an unavailable engine and a failed save", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(
      settings({ torrentEngine: "unavailable", downloadPort: null, sharingPort: null }),
    );
    vi.mocked(libraryApi.setNetworkProfile).mockRejectedValue({ code: "settings_failed", message: "the setting could not be stored" });
    const { view } = renderSettings();

    await waitFor(() => expect(view.container.textContent).toContain("The torrent engine did not start"));
    fireEvent.click(radio(view.container, "loopback")!);
    await waitFor(() => expect(view.container.textContent).toContain("the setting could not be stored"));
  });

  it("returns to the previous view", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view, onBack } = renderSettings();

    const back = Array.from(view.container.querySelectorAll("button")).find((button) => button.textContent?.includes("Back"));
    fireEvent.click(back!);
    expect(onBack).toHaveBeenCalled();
  });
});
