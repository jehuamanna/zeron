import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type Backup } from "../../services/library";
import { BackupSettings } from "./BackupSettings";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { exportBackup: vi.fn(), chooseBackupToRestore: vi.fn(), pendingRestore: vi.fn(), cancelRestore: vi.fn(), restartApp: vi.fn() },
}));

const backup: Backup = { createdAtUnixSeconds: 1_760_000_000, contents: ["library", "publish"], totalBytes: 2 * 1024 * 1024 };

function renderSettings() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><BackupSettings /></QueryClientProvider>);
}

beforeEach(() => {
  vi.mocked(libraryApi.pendingRestore).mockResolvedValue(null);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("backup", () => {
  it("saves a backup and says what it holds", async () => {
    vi.mocked(libraryApi.exportBackup).mockResolvedValueOnce(null).mockResolvedValueOnce(backup);
    const view = renderSettings();
    fireEvent.click(view.getByRole("button", { name: "Save backup…" }));
    await waitFor(() => expect(libraryApi.exportBackup).toHaveBeenCalledTimes(1));
    expect(view.queryByRole("status")).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "Save backup…" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("your library, notes, and progress and your Publish drafts"));
    expect(view.container.textContent).toContain("does not hold course videos, release files, or your signing key");
  });

  it("confirms, stages a restore, and offers a restart or to keep the current data", async () => {
    vi.mocked(libraryApi.chooseBackupToRestore).mockImplementation(async () => {
      vi.mocked(libraryApi.pendingRestore).mockResolvedValue(backup);
      return backup;
    });
    vi.mocked(libraryApi.cancelRestore).mockImplementation(async () => {
      vi.mocked(libraryApi.pendingRestore).mockResolvedValue(null);
    });
    const view = renderSettings();
    fireEvent.click(view.getByRole("button", { name: /Restore from backup/ }));
    const dialog = view.getByRole("alertdialog", { name: "Restore from a backup?" });
    expect(dialog.textContent).toContain("before-restore");
    fireEvent.click(within(dialog).getByRole("button", { name: "Choose backup…" }));
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Restart Zeron to restore it."));
    fireEvent.click(view.getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(libraryApi.restartApp).toHaveBeenCalled());
    fireEvent.click(view.getByRole("button", { name: "Don't restore" }));
    await waitFor(() => expect(view.queryByRole("status")).toBeNull());
  });

  it("explains a backup from a newer Zeron", async () => {
    vi.mocked(libraryApi.chooseBackupToRestore).mockRejectedValue({ code: "backup_too_new", message: "this backup was made by a newer version of Zeron; update Zeron first" });
    const view = renderSettings();
    fireEvent.click(view.getByRole("button", { name: /Restore from backup/ }));
    fireEvent.click(view.getByRole("button", { name: "Choose backup…" }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("update Zeron first"));
  });
});
