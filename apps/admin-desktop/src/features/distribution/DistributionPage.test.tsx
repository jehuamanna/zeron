import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type Distribution, type SeedingStatus } from "../../services/publisher";
import { DistributionPage } from "./DistributionPage";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { getDistribution: vi.fn(), startSeeding: vi.fn(), stopSeeding: vi.fn(), setNetworkProfile: vi.fn() },
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
  server = { configuredProfile: "local_network", activeProfile: "local_network", listenPort: 42_001, engineError: null, releases: [status({})] };
  vi.mocked(publisherApi.getDistribution).mockImplementation(async () => server);
  vi.mocked(publisherApi.startSeeding).mockImplementation(async () => {
    server = { ...server, releases: [status({ enabled: true, state: "seeding", verifiedBytes: 2_097_152, uploadedBytes: 3_145_728, uploadBytesPerSecond: 1024, connectedPeers: 2 })] };
  });
  vi.mocked(publisherApi.stopSeeding).mockImplementation(async () => {
    server = { ...server, releases: [status({})] };
  });
  vi.mocked(publisherApi.setNetworkProfile).mockImplementation(async (profile) => {
    server = { ...server, configuredProfile: profile };
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

  it("saves the network profile and asks for a restart", async () => {
    const view = renderPage();
    const internet = await waitFor(() => view.getByRole("radio", { name: /Internet/ }));
    expect((view.getByRole("radio", { name: /Local network/ }) as HTMLInputElement).checked).toBe(true);
    fireEvent.click(internet);
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Restart Zeron Publisher"));
    expect(publisherApi.setNetworkProfile).toHaveBeenCalledWith("internet");
    expect(view.container.textContent).toContain("Your IP address becomes visible");
  });

  it("explains when the engine is unavailable", async () => {
    server = { ...server, engineError: "address in use", listenPort: null };
    const view = renderPage();
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("address in use"));
    expect((view.getByRole("button", { name: /Start seeding/ }) as HTMLButtonElement).disabled).toBe(true);
  });
});
