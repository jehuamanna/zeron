import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type ConfigurationPreview } from "../../services/publisher";
import { ConfigurationSection, describeNetwork } from "./ConfigurationSection";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: {
    exportPublisherConfiguration: vi.fn(),
    choosePublisherConfiguration: vi.fn(),
    applyPublisherConfiguration: vi.fn(),
    discardPublisherConfiguration: vi.fn(),
  },
}));

const preview: ConfigurationPreview = {
  token: "configuration_1",
  trackers: ["udp://tracker.example:6969/announce"],
  trackerLists: ["https://lists.example/best.txt"],
  network: { profile: "internet", peerPort: 51413, portForwarding: true, downloadLimitKib: null, uploadLimitKib: 512 },
};

function renderSection() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><ConfigurationSection /></QueryClientProvider>);
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("publisher configuration", () => {
  it("describes network settings in words", () => {
    expect(describeNetwork(preview.network!)).toBe("Internet, port 51413, router forwarding on, download unlimited, upload 512 KiB/s");
    expect(describeNetwork({ ...preview.network!, profile: "local_network", peerPort: null, portForwarding: false })).toContain("Local network, a new port each start, router forwarding off");
  });

  it("exports with or without the Network settings", async () => {
    vi.mocked(publisherApi.exportPublisherConfiguration).mockResolvedValue(true);
    const view = renderSection();
    fireEvent.click(view.getByRole("button", { name: /Export/ }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Configuration saved."));
    expect(publisherApi.exportPublisherConfiguration).toHaveBeenCalledWith(true);
    fireEvent.click(view.getByLabelText("Include Network settings"));
    fireEvent.click(view.getByRole("button", { name: /Export/ }));
    await waitFor(() => expect(publisherApi.exportPublisherConfiguration).toHaveBeenLastCalledWith(false));
  });

  it("reviews an imported file before applying it, network included", async () => {
    vi.mocked(publisherApi.choosePublisherConfiguration).mockResolvedValue(preview);
    vi.mocked(publisherApi.applyPublisherConfiguration).mockResolvedValue({ trackers: 1, trackerLists: 1, networkApplied: true, restartRequired: true });
    const view = renderSection();
    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    const review = await waitFor(() => view.getByRole("group", { name: "Review imported configuration" }));
    expect(review.textContent).toContain("udp://tracker.example:6969/announce");
    expect(review.textContent).toContain("Internet, port 51413");
    expect(publisherApi.applyPublisherConfiguration).not.toHaveBeenCalled();
    fireEvent.click(within(review).getByRole("button", { name: "Apply configuration" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Restart Zeron to use the new network profile and port."));
    expect(publisherApi.applyPublisherConfiguration).toHaveBeenCalledWith("configuration_1", true);
    expect(view.queryByRole("group", { name: "Review imported configuration" })).toBeNull();
  });

  it("can leave the Network settings out and can cancel", async () => {
    vi.mocked(publisherApi.choosePublisherConfiguration).mockResolvedValue(preview);
    vi.mocked(publisherApi.applyPublisherConfiguration).mockResolvedValue({ trackers: 1, trackerLists: 1, networkApplied: false, restartRequired: false });
    const view = renderSection();
    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    await waitFor(() => view.getByRole("group", { name: "Review imported configuration" }));
    fireEvent.click(view.getByLabelText("Also apply these Network settings"));
    fireEvent.click(view.getByRole("button", { name: "Apply configuration" }));
    await waitFor(() => expect(publisherApi.applyPublisherConfiguration).toHaveBeenCalledWith("configuration_1", false));
    await waitFor(() => expect(view.getByRole("status").textContent).not.toContain("Restart"));

    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    await waitFor(() => view.getByRole("group", { name: "Review imported configuration" }));
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    expect(publisherApi.discardPublisherConfiguration).toHaveBeenCalledWith("configuration_1");
    expect(view.queryByRole("group", { name: "Review imported configuration" })).toBeNull();
  });

  it("explains a file that cannot be imported and ignores a cancelled picker", async () => {
    vi.mocked(publisherApi.choosePublisherConfiguration).mockResolvedValueOnce(null).mockRejectedValueOnce({ code: "invalid_input", message: "configuration is not a Zeron publisher configuration", field: "configuration" });
    const view = renderSection();
    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    await waitFor(() => expect(publisherApi.choosePublisherConfiguration).toHaveBeenCalledTimes(1));
    expect(view.queryByRole("group")).toBeNull();
    fireEvent.click(view.getByRole("button", { name: /Import/ }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("is not a Zeron publisher configuration"));
  });
});
