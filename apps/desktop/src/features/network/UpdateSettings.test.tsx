import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../../services/library";
import { accessibilityViolations } from "../../test/accessibility";
import { UpdateSettings } from "./UpdateSettings";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { checkForUpdate: vi.fn(), installUpdate: vi.fn() },
}));

function renderSettings() {
  return render(<QueryClientProvider client={new QueryClient()}><UpdateSettings /></QueryClientProvider>);
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("updates", () => {
  it("checks only when asked and reports an up-to-date app", async () => {
    vi.mocked(libraryApi.checkForUpdate).mockResolvedValue(null);
    const view = renderSettings();
    expect(libraryApi.checkForUpdate).not.toHaveBeenCalled();
    fireEvent.click(view.getByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("up to date"));
  });

  it("offers a newer version and installs it", async () => {
    vi.mocked(libraryApi.checkForUpdate).mockResolvedValue({ version: "0.2.0", currentVersion: "0.1.0", notes: "Signed catalogs" });
    vi.mocked(libraryApi.installUpdate).mockResolvedValue();
    const view = renderSettings();
    fireEvent.click(view.getByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Zeron 0.2.0 is available (you have 0.1.0)."));
    expect(await accessibilityViolations(view.container)).toEqual([]);
    fireEvent.click(view.getByRole("button", { name: "Install 0.2.0 and restart" }));
    await waitFor(() => expect(libraryApi.installUpdate).toHaveBeenCalled());
  });

  it("refuses an update that is not signed by the release key", async () => {
    vi.mocked(libraryApi.checkForUpdate).mockResolvedValue({ version: "0.2.0", currentVersion: "0.1.0", notes: null });
    vi.mocked(libraryApi.installUpdate).mockRejectedValue({ code: "update_not_trusted", message: "the update is not signed by Zeron's release key, so it was not installed" });
    const view = renderSettings();
    fireEvent.click(view.getByRole("button", { name: "Check for updates" }));
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: "Install 0.2.0 and restart" })));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("not signed by Zeron's release key"));
  });
});
