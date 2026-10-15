import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type Distribution, type SeedingStatus } from "../../services/publisher";
import { DistributionPage } from "./DistributionPage";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { getDistribution: vi.fn(), startSeeding: vi.fn(), stopSeeding: vi.fn(), setNetworkProfile: vi.fn(), setPeerReachability: vi.fn(), setUploadLimit: vi.fn(), setTrackers: vi.fn(), setTrackerLists: vi.fn(), refreshTrackerLists: vi.fn() },
}));

function status(overrides: Partial<SeedingStatus>): SeedingStatus {
  return {
    releaseId: "release_one",
    courseVersion: "1.0.0",
    title: "Rust Systems",
    releaseStatus: "published",
    enabled: false,
    state: null,
    verifiedBytes: 0,
    totalBytes: 2_097_152,
    uploadedBytes: 0,
    uploadBytesPerSecond: 0,
    connectedPeers: 0,
    failure: null,
    ...overrides,
  };
}

let server: Distribution;

beforeEach(() => {
  server = {
    configuredProfile: "local_network",
    activeProfile: "local_network",
    listenPort: 42_001,
    peerPort: null,
    portForwarding: false,
    activePortForwarding: false,
    restartRequired: false,
    trackers: [],
    trackerLists: [
      { url: "https://lists.example/best.txt", trackerCount: 20, fetchedAtUnixSeconds: Math.floor(Date.now() / 1000) - 300 },
      { url: "https://other.example/best.txt", trackerCount: 0, fetchedAtUnixSeconds: null },
    ],
    effectiveTrackerCount: 20,
    uploadLimitKib: null,
    engineError: null,
    releases: [status({})],
  };
  vi.mocked(publisherApi.getDistribution).mockImplementation(async () => server);
  vi.mocked(publisherApi.startSeeding).mockImplementation(async () => {
    server = { ...server, releases: [status({ enabled: true, state: "seeding", verifiedBytes: 2_097_152, uploadedBytes: 3_145_728, uploadBytesPerSecond: 1024, connectedPeers: 2 })] };
  });
  vi.mocked(publisherApi.stopSeeding).mockImplementation(async () => {
    server = { ...server, releases: [status({})] };
  });
  vi.mocked(publisherApi.setNetworkProfile).mockImplementation(async (profile) => {
    server = { ...server, configuredProfile: profile, restartRequired: true };
  });
  vi.mocked(publisherApi.setTrackers).mockImplementation(async (trackers) => {
    server = { ...server, trackers, effectiveTrackerCount: server.effectiveTrackerCount + trackers.length };
    return trackers;
  });
  vi.mocked(publisherApi.setTrackerLists).mockImplementation(async (urls) => {
    server = { ...server, trackerLists: urls.map((url) => ({ url, trackerCount: 0, fetchedAtUnixSeconds: null })) };
    return urls;
  });
  vi.mocked(publisherApi.refreshTrackerLists).mockImplementation(async () => {
    server = {
      ...server,
      trackerLists: server.trackerLists.map((list) => ({ ...list, trackerCount: 7, fetchedAtUnixSeconds: Math.floor(Date.now() / 1000) })),
      effectiveTrackerCount: 7,
    };
    return server.trackerLists.map((list) => ({ url: list.url, trackerCount: 7, error: null }));
  });
  vi.mocked(publisherApi.setPeerReachability).mockImplementation(async (peerPort, portForwarding) => {
    server = { ...server, peerPort, portForwarding, restartRequired: true };
  });
});
afterEach(cleanup);

function renderPage() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><DistributionPage /></QueryClientProvider>);
}

describe("distribution workspace", () => {
  it("starts and stops seeding with local metrics", async () => {
    const view = renderPage();
    const start = await waitFor(() => view.getByRole("button", { name: "Start seeding Rust Systems 1.0.0" }));
    expect(view.container.textContent).toContain("not download counts");
    expect(view.container.textContent).toContain("peers connect on port 42001");
    fireEvent.click(start);
    await waitFor(() => expect(view.container.textContent).toContain("2 peers connected"));
    expect(publisherApi.startSeeding).toHaveBeenCalledWith("release_one");
    expect(view.container.textContent).toContain("3.0 MB uploaded this session");
    expect(view.container.textContent).toContain("Seeding");
    fireEvent.click(view.getByRole("button", { name: "Stop seeding Rust Systems 1.0.0" }));
    await waitFor(() => expect(view.getByRole("button", { name: "Start seeding Rust Systems 1.0.0" })).toBeTruthy());
    expect(publisherApi.stopSeeding).toHaveBeenCalledWith("release_one");
  });

  it("shows checking progress and fail-closed reasons", async () => {
    server = {
      ...server,
      releases: [
        status({ releaseId: "a", enabled: true, state: "checking", verifiedBytes: 1_048_576 }),
        status({ releaseId: "b", courseVersion: "0.9.0", enabled: true, state: "failed", failure: "the content on disk no longer matches the torrent; nothing is being served" }),
      ],
    };
    const view = renderPage();
    await waitFor(() => expect(view.container.textContent).toContain("Checking 50%"));
    expect(view.container.textContent).toContain("Verifying release files before serving them");
    expect(view.getByRole("alert").textContent).toContain("nothing is being served");
    expect(view.container.textContent).toContain("Not serving");
  });

  it("saves an upload limit and rejects values that are too small", async () => {
    vi.mocked(publisherApi.setUploadLimit).mockImplementation(async (uploadKib) => {
      server = { ...server, uploadLimitKib: uploadKib };
    });
    const view = renderPage();
    const field = await waitFor(() => view.getByLabelText(/Upload limit/) as HTMLInputElement);
    expect(field.value).toBe("");
    fireEvent.change(field, { target: { value: "4" } });
    fireEvent.click(view.getByRole("button", { name: "Save upload limit" }));
    expect(view.getByRole("alert").textContent).toContain("at least 16 KiB/s");
    expect(publisherApi.setUploadLimit).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "512" } });
    fireEvent.click(view.getByRole("button", { name: "Save upload limit" }));
    await waitFor(() => expect(publisherApi.setUploadLimit).toHaveBeenCalledWith(512));
    await waitFor(() => expect((view.getByLabelText(/Upload limit/) as HTMLInputElement).value).toBe("512"));
  });

  it("does not offer to seed archived releases", async () => {
    server = { ...server, releases: [status({ releaseStatus: "archived" })] };
    const view = renderPage();
    const start = await waitFor(() => view.getByRole("button", { name: "Start seeding Rust Systems 1.0.0" }));
    expect(start.hasAttribute("disabled")).toBe(true);
    expect(start.getAttribute("title")).toBe("Archived releases are not seeded");
    expect(view.container.textContent).toContain("archived");
  });

  it("saves the network profile and asks for a restart", async () => {
    const view = renderPage();
    const internet = await waitFor(() => view.getByRole("radio", { name: /Internet/ }));
    expect((view.getByRole("radio", { name: /Local network/ }) as HTMLInputElement).checked).toBe(true);
    fireEvent.click(internet);
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Restart Zeron Publisher"));
    expect(publisherApi.setNetworkProfile).toHaveBeenCalledWith("internet");
    expect(view.container.textContent).toContain("Your IP address becomes visible");
  });

  it("saves a fixed port with UPnP under the internet profile", async () => {
    server = { ...server, configuredProfile: "internet", activeProfile: "internet" };
    const view = renderPage();
    const port = await waitFor(() => view.getByLabelText("Peer port"));
    expect(view.container.textContent).toContain("UPnP is off.");
    fireEvent.change(port, { target: { value: "51413" } });
    fireEvent.click(view.getByRole("checkbox", { name: /forward the port/ }));
    fireEvent.click(view.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Restart Zeron Publisher"));
    expect(publisherApi.setPeerReachability).toHaveBeenCalledWith(51413, true);
    expect((view.getByLabelText("Peer port") as HTMLInputElement).value).toBe("51413");
  });

  it("keeps UPnP off outside the internet profile and rejects bad ports", async () => {
    const view = renderPage();
    const port = await waitFor(() => view.getByLabelText("Peer port"));
    expect((view.getByRole("checkbox", { name: /forward the port/ }) as HTMLInputElement).disabled).toBe(true);
    fireEvent.change(port, { target: { value: "443" } });
    fireEvent.click(view.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("Choose a port from 1024 to 65535"));
    expect(publisherApi.setPeerReachability).not.toHaveBeenCalled();
  });

  it("reports active UPnP forwarding", async () => {
    server = { ...server, configuredProfile: "internet", activeProfile: "internet", peerPort: 51413, portForwarding: true, activePortForwarding: true, listenPort: 51413 };
    const view = renderPage();
    await waitFor(() => expect(view.container.textContent).toContain("Your router is asked to forward this port with UPnP."));
    expect(view.container.textContent).toContain("peers connect on port 51413");
  });

  it("saves trackers one per line and explains the privacy trade-off", async () => {
    const view = renderPage();
    const field = await waitFor(() => view.getByLabelText("Additional tracker URLs, one per line"));
    expect(view.container.textContent).toContain("shows the IP address of everyone downloading");
    expect(view.container.textContent).toContain("Only the Internet profile announces to trackers.");
    expect((view.getByRole("button", { name: "Save trackers" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(field, { target: { value: " udp://tracker.example:6969/announce \n\nhttps://tracker.example/announce\n" } });
    fireEvent.click(view.getByRole("button", { name: "Save trackers" }));
    await waitFor(() => expect(publisherApi.setTrackers).toHaveBeenCalledWith(["udp://tracker.example:6969/announce", "https://tracker.example/announce"]));
    await waitFor(() => expect((view.getByRole("button", { name: "Save trackers" }) as HTMLButtonElement).disabled).toBe(true));
    expect((view.getByLabelText("Additional tracker URLs, one per line") as HTMLTextAreaElement).value).toBe(
      "udp://tracker.example:6969/announce\nhttps://tracker.example/announce",
    );
  });

  it("lists each followed tracker list with its last download and the total", async () => {
    const view = renderPage();
    await waitFor(() => expect(view.getByRole("list", { name: "Tracker list status" })).toBeTruthy());
    const status = view.getByRole("list", { name: "Tracker list status" }).textContent;
    expect(status).toContain("20 trackers · downloaded 5 min ago");
    expect(status).toContain("0 trackers · not downloaded yet");
    expect((view.getByLabelText("Tracker list URLs, one per line") as HTMLTextAreaElement).value).toBe(
      "https://lists.example/best.txt\nhttps://other.example/best.txt",
    );
    expect(view.container.textContent).toContain("New releases will carry 20 trackers: yours first, then the lists.");
  });

  it("saves an added list URL and downloads the lists again", async () => {
    const view = renderPage();
    const field = await waitFor(() => view.getByLabelText("Tracker list URLs, one per line"));
    fireEvent.change(field, { target: { value: "https://lists.example/best.txt\nhttps://other.example/best.txt\nhttps://mine.example/list.txt" } });
    fireEvent.click(view.getByRole("button", { name: "Save lists" }));
    await waitFor(() => expect(publisherApi.refreshTrackerLists).toHaveBeenCalled());
    expect(publisherApi.setTrackerLists).toHaveBeenCalledWith(["https://lists.example/best.txt", "https://other.example/best.txt", "https://mine.example/list.txt"]);
    await waitFor(() => expect(view.container.textContent).toContain("New releases will carry 7 trackers"));
    expect(view.getByRole("list", { name: "Tracker list status" }).textContent).toContain("mine.example/list.txt7 trackers · downloaded just now");
  });

  it("reports a list that failed to refresh", async () => {
    vi.mocked(publisherApi.refreshTrackerLists).mockResolvedValue([
      { url: "https://lists.example/best.txt", trackerCount: 20, error: null },
      { url: "https://other.example/best.txt", trackerCount: null, error: "the download timed out" },
    ]);
    const view = renderPage();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Refresh lists" })));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("Refresh failed: the download timed out"));
  });

  it("shows why trackers were rejected", async () => {
    vi.mocked(publisherApi.setTrackers).mockRejectedValue({ code: "invalid_input", field: "trackers", message: "trackers must be http, https, or udp URLs with a host" });
    const view = renderPage();
    const field = await waitFor(() => view.getByLabelText("Additional tracker URLs, one per line"));
    fireEvent.change(field, { target: { value: "ftp://tracker.example" } });
    fireEvent.click(view.getByRole("button", { name: "Save trackers" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("must be http, https, or udp URLs"));
  });

  it("explains when the engine is unavailable", async () => {
    server = { ...server, engineError: "address in use", listenPort: null };
    const view = renderPage();
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("address in use"));
    expect((view.getByRole("button", { name: /Start seeding/ }) as HTMLButtonElement).disabled).toBe(true);
  });
});
