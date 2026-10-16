import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { libraryApi, type LearnerKey } from "../../services/library";
import { accessibilityViolations } from "../../test/accessibility";
import { LearnerKeySettings } from "./LearnerKeySettings";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { getLearnerKey: vi.fn(), createLearnerKey: vi.fn(), exportLearnerKeyBackup: vi.fn(), restoreLearnerKeyBackup: vi.fn() },
}));

const none: LearnerKey = { status: "none", signingKey: null, encryptionKey: null, fingerprint: null, learnerId: null, createdAtUnixSeconds: null, problem: null };
const ready: LearnerKey = {
  status: "ready",
  signingKey: `ed25519-${"a".repeat(64)}`,
  encryptionKey: `x25519-${"b".repeat(64)}`,
  fingerprint: "7F3A-91C2-0B44-E8D1",
  learnerId: `zeron-learner:v1:${"a".repeat(64)}:${"b".repeat(64)}`,
  createdAtUnixSeconds: 1_760_000_000,
  problem: null,
};

function renderSettings() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><LearnerKeySettings /></QueryClientProvider>);
}

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("learner key", () => {
  it("creates the key and shows its fingerprint", async () => {
    vi.mocked(libraryApi.getLearnerKey).mockResolvedValue(none);
    vi.mocked(libraryApi.createLearnerKey).mockResolvedValue(ready);
    const view = renderSettings();
    fireEvent.click(await view.findByRole("button", { name: "Create learner key" }));
    await waitFor(() => expect(view.container.textContent).toContain("7F3A-91C2-0B44-E8D1"));
    expect(view.container.textContent).toContain("Ready");
    expect(view.container.textContent).toContain("no account or password");
    expect(await accessibilityViolations(view.container)).toEqual([]);
  });

  it("shows the learner ID to share and copies it", async () => {
    vi.mocked(libraryApi.getLearnerKey).mockResolvedValue(ready);
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const view = renderSettings();
    const field = (await view.findByLabelText("Learner ID")) as HTMLInputElement;
    expect(field.value).toBe(ready.learnerId);
    expect(field.readOnly).toBe(true);
    fireEvent.click(view.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(ready.learnerId));
    expect(await view.findByRole("button", { name: "Copied" })).toBeTruthy();
  });

  it("backs up only with a long, repeated passphrase", async () => {
    vi.mocked(libraryApi.getLearnerKey).mockResolvedValue(ready);
    vi.mocked(libraryApi.exportLearnerKeyBackup).mockResolvedValue(true);
    const view = renderSettings();
    const save = await view.findByRole("button", { name: /Save backup/ });
    const passphrase = view.getByLabelText("Learner key backup passphrase");
    const repeat = view.getByLabelText("Repeat learner key backup passphrase");
    fireEvent.change(passphrase, { target: { value: "short" } });
    expect(view.container.textContent).toContain("at least 12 characters");
    expect(save).toHaveProperty("disabled", true);
    fireEvent.change(passphrase, { target: { value: "correct horse battery" } });
    fireEvent.change(repeat, { target: { value: "correct horse battery" } });
    expect(save).toHaveProperty("disabled", false);
    fireEvent.click(save);
    await waitFor(() => expect(libraryApi.exportLearnerKeyBackup).toHaveBeenCalledWith("correct horse battery"));
    expect((await view.findByRole("status")).textContent).toContain("Backup saved");
  });

  it("explains a missing key and restores it from a backup", async () => {
    vi.mocked(libraryApi.getLearnerKey).mockResolvedValue({ ...ready, status: "key_missing", problem: "this computer's keychain has no learner key" });
    vi.mocked(libraryApi.restoreLearnerKeyBackup).mockRejectedValueOnce({ code: "wrong_passphrase", message: "the passphrase is wrong, or the backup was changed" }).mockResolvedValueOnce(ready);
    const view = renderSettings();
    await waitFor(() => expect(view.container.textContent).toContain("no longer has this key"));
    expect(view.getByRole("button", { name: /Save backup/ })).toHaveProperty("disabled", true);
    fireEvent.change(view.getByLabelText("Learner key restore passphrase"), { target: { value: "correct horse battery" } });
    fireEvent.click(view.getByRole("button", { name: /Choose backup/ }));
    expect((await view.findByRole("alert")).textContent).toContain("passphrase is wrong");
    fireEvent.change(view.getByLabelText("Learner key restore passphrase"), { target: { value: "correct horse battery" } });
    fireEvent.click(view.getByRole("button", { name: /Choose backup/ }));
    await waitFor(() => expect(view.container.textContent).toContain("Key restored"));
    expect(view.container.textContent).toContain("Ready");
  });
});
