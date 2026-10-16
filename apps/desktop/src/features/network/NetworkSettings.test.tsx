import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type NetworkSettings as Settings } from "../../services/library";
import { NetworkSettings } from "./NetworkSettings";
import { accessibilityViolations } from "../../test/accessibility";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: {
    pendingRestore: vi.fn(() => Promise.resolve(null)),
    getNetworkSettings: vi.fn(),
    setNetworkProfile: vi.fn(),
    setPeerReachability: vi.fn(),
    setRateLimits: vi.fn(),
    storage: vi.fn(),
    setCacheLimit: vi.fn(),
  },
}));

const settings = (overrides: Partial<Settings>): Settings => ({
  configuredProfile: "local_network",
  activeProfile: "local_network",
  torrentEngine: "ready",
  downloadPort: 41213,
  sharingPort: 41099,
  peerPort: null,
  portForwarding: false,
  activePortForwarding: false,
  restartRequired: false,
  downloadLimitKib: null,
  uploadLimitKib: null,
  portMapping: null,
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

afterEach(cleanup);

describe("network settings", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows the active local-network profile and this session's ports", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view } = renderSettings();

    await waitFor(() => expect(radio(view.container, "local_network")?.checked).toBe(true));
    expect(radio(view.container, "internet")?.checked).toBe(false);
    expect(view.container.textContent).toContain("downloads use port 41213");
    expect(view.container.textContent).toContain("shared courses and releases are served on port 41099");
    expect(view.queryByRole("status")).toBeNull();
    expect(view.container.textContent).not.toContain("Restart Zeron");
  });

  it("saves the internet profile and asks for a restart", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValueOnce(settings({}));
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({ configuredProfile: "internet", restartRequired: true }));
    vi.mocked(libraryApi.setNetworkProfile).mockResolvedValue();
    const { view } = renderSettings();

    await waitFor(() => expect(radio(view.container, "internet")).not.toBeNull());
    expect(view.container.textContent).toContain("Your IP address becomes visible to other DHT and tracker users");
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

  it("offers UPnP only with the internet profile", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view } = renderSettings();

    await waitFor(() => expect(view.container.querySelector("#peer-port")).not.toBeNull());
    const upnp = view.container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(upnp.disabled).toBe(true);
    expect(view.container.textContent).toContain("Available with the Internet profile.");
  });

  it("saves a fixed port with UPnP and asks for a restart", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValueOnce(
      settings({ configuredProfile: "internet", activeProfile: "internet" }),
    );
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(
      settings({ configuredProfile: "internet", activeProfile: "internet", peerPort: 51413, portForwarding: true, restartRequired: true }),
    );
    vi.mocked(libraryApi.setPeerReachability).mockResolvedValue();
    const { view } = renderSettings();

    await waitFor(() => expect(view.container.querySelector("#peer-port")).not.toBeNull());
    fireEvent.change(view.container.querySelector("#peer-port")!, { target: { value: "51413" } });
    fireEvent.click(view.container.querySelector('input[type="checkbox"]')!);
    fireEvent.click(Array.from(view.container.querySelectorAll("button")).find((button) => button.textContent === "Save")!);
    await waitFor(() => expect(view.container.textContent).toContain("Restart Zeron to apply the new network setting."));
    expect(libraryApi.setPeerReachability).toHaveBeenCalledWith(51413, true);
    expect(view.container.querySelector<HTMLInputElement>("#peer-port")?.value).toBe("51413");
  });

  it("rejects a privileged or malformed port without saving", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view } = renderSettings();

    await waitFor(() => expect(view.container.querySelector("#peer-port")).not.toBeNull());
    for (const value of ["80", "70000", "12ab"]) {
      fireEvent.change(view.container.querySelector("#peer-port")!, { target: { value } });
      fireEvent.click(Array.from(view.container.querySelectorAll("button")).find((button) => button.textContent === "Save")!);
      await waitFor(() => expect(view.container.textContent).toContain("Choose a port from 1024 to 65535"));
    }
    expect(libraryApi.setPeerReachability).not.toHaveBeenCalled();
  });

  it("reports what the router did and a sharing engine that could not take its port", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(
      settings({
        configuredProfile: "internet",
        activeProfile: "internet",
        peerPort: 51413,
        portForwarding: true,
        activePortForwarding: true,
        sharingPort: null,
        portMapping: {
          state: "unavailable",
          protocol: null,
          ports: [],
          tried: [
            { protocol: "pcp", outcome: "no_answer", code: null },
            { protocol: "nat_pmp", outcome: "no_answer", code: null },
            { protocol: "upnp", outcome: "no_answer", code: null },
          ],
        },
      }),
    );
    const { view } = renderSettings();

    await waitFor(() =>
      expect(view.getByRole("status").textContent).toBe("No router answered UPnP, NAT-PMP, or PCP. Forward TCP and UDP 51413 by hand for peers to reach you."),
    );
    expect(view.container.textContent).toContain("Sharing and seeding could not start, possibly because port 51413 is in use");
  });

  it("follows the router's answer while Zeron asks", async () => {
    const probing = settings({
      configuredProfile: "internet",
      activeProfile: "internet",
      peerPort: 51413,
      sharingPort: 51413,
      portForwarding: true,
      activePortForwarding: true,
      portMapping: { state: "probing", protocol: null, ports: [], tried: [] },
    });
    vi.mocked(libraryApi.getNetworkSettings)
      .mockResolvedValueOnce(probing)
      .mockResolvedValue({
        ...probing,
        portMapping: {
          state: "mapped",
          protocol: "pcp",
          ports: [
            { internalPort: 51413, externalPort: 51413, transport: "tcp" },
            { internalPort: 51413, externalPort: 51413, transport: "udp" },
          ],
          tried: [{ protocol: "pcp", outcome: "mapped", code: null }],
        },
      });
    const { view } = renderSettings();

    await waitFor(() => expect(view.getByRole("status").textContent).toBe("Asking your router to open port 51413…"));
    await waitFor(() => expect(view.getByRole("status").textContent).toBe("Router: port 51413 open via PCP (TCP and UDP)."), { timeout: 3000 });
  });

  it("returns to the previous view", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    const { view, onBack } = renderSettings();

    const back = Array.from(view.container.querySelectorAll("button")).find((button) => button.textContent?.includes("Back"));
    fireEvent.click(back!);
    expect(onBack).toHaveBeenCalled();
  });

  it("saves speed limits, treats empty fields as no limit, and rejects tiny values", async () => {
    let current = settings({ uploadLimitKib: 256 });
    vi.mocked(libraryApi.getNetworkSettings).mockImplementation(async () => current);
    vi.mocked(libraryApi.setRateLimits).mockImplementation(async (downloadKib, uploadKib) => {
      current = { ...current, downloadLimitKib: downloadKib, uploadLimitKib: uploadKib };
    });
    const { view } = renderSettings();
    const download = await waitFor(() => view.getByLabelText("Download limit") as HTMLInputElement);
    const upload = view.getByLabelText("Upload limit (sharing and seeding)") as HTMLInputElement;
    expect(download.value).toBe("");
    expect(upload.value).toBe("256");

    fireEvent.change(download, { target: { value: "8" } });
    fireEvent.click(view.getByRole("button", { name: "Save limits" }));
    expect(view.getByRole("alert").textContent).toContain("at least 16 KiB/s");
    expect(libraryApi.setRateLimits).not.toHaveBeenCalled();

    fireEvent.change(download, { target: { value: "2048" } });
    fireEvent.change(upload, { target: { value: " " } });
    fireEvent.click(view.getByRole("button", { name: "Save limits" }));
    await waitFor(() => expect(libraryApi.setRateLimits).toHaveBeenCalledWith(2048, null));
    await waitFor(() => expect((view.getByLabelText("Download limit") as HTMLInputElement).value).toBe("2048"));
    expect((view.getByLabelText("Upload limit (sharing and seeding)") as HTMLInputElement).value).toBe("");
  });

  it("shows streamed-lesson storage and saves a cap in GB", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({}));
    let limit: number | null = null;
    vi.mocked(libraryApi.storage).mockImplementation(async () => ({ streamedBytes: 1.5 * 1024 ** 3, cacheLimitBytes: limit, availableBytes: 20 * 1024 ** 3, lowDisk: false }));
    vi.mocked(libraryApi.setCacheLimit).mockImplementation(async (bytes) => {
      limit = bytes;
      return { removed: ["a"], freedBytes: 300 * 1024 ** 2, kept: [] };
    });
    const { view } = renderSettings();
    await waitFor(() => expect(view.container.textContent).toContain("use 1.5 GB"));
    expect(view.container.textContent).toContain("20.0 GB free on the downloads disk");
    const field = view.getByLabelText("Limit for streamed lessons (GB)") as HTMLInputElement;
    fireEvent.change(field, { target: { value: "0.1" } });
    fireEvent.click(view.getByRole("button", { name: "Save limit" }));
    expect(view.getByRole("alert").textContent).toContain("at least 0.25 GB");
    expect(libraryApi.setCacheLimit).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "2" } });
    fireEvent.click(view.getByRole("button", { name: "Save limit" }));
    await waitFor(() => expect(libraryApi.setCacheLimit).toHaveBeenCalledWith(2 * 1024 ** 3));
    await waitFor(() => expect((view.getByLabelText("Limit for streamed lessons (GB)") as HTMLInputElement).value).toBe("2"));
  });

  it("has no accessibility violations", async () => {
    vi.mocked(libraryApi.getNetworkSettings).mockResolvedValue(settings({ configuredProfile: "internet", activeProfile: "internet", restartRequired: true }));
    const { view } = renderSettings();
    await waitFor(() => expect(radio(view.container, "internet")?.checked).toBe(true));
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });
});
