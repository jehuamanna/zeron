import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type PublisherIdentity } from "../../services/publisher";
import { IdentityPage } from "./IdentityPage";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: {
    getIdentity: vi.fn(),
    createIdentity: vi.fn(),
    rotateIdentity: vi.fn(),
    setSignReleases: vi.fn(),
    exportIdentityBackup: vi.fn(),
    restoreIdentityBackup: vi.fn(),
  },
}));

const none: PublisherIdentity = { status: "none", publisherId: null, signingKey: null, fingerprint: null, createdAtUnixSeconds: null, rotations: 0, signReleases: true, problem: null };
const ready: PublisherIdentity = { status: "ready", publisherId: `ed25519-${"a".repeat(64)}`, signingKey: `ed25519-${"a".repeat(64)}`, fingerprint: "7F3A-91C2-0B44-E8D1", createdAtUnixSeconds: 1_760_000_000, rotations: 0, signReleases: true, problem: null };

function renderPage() {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><IdentityPage /></QueryClientProvider>);
}

beforeEach(() => {
  vi.mocked(publisherApi.getIdentity).mockResolvedValue(ready);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("publisher identity", () => {
  it("creates an identity and shows its fingerprint", async () => {
    vi.mocked(publisherApi.getIdentity).mockResolvedValue(none);
    vi.mocked(publisherApi.createIdentity).mockResolvedValue(ready);
    const view = renderPage();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /Create signing identity/ })));
    await waitFor(() => expect(view.container.textContent).toContain("Ready to sign"));
    expect(view.container.textContent).toContain("7F3A-91C2-0B44-E8D1");
  });

  it("explains a missing key and a locked keychain", async () => {
    vi.mocked(publisherApi.getIdentity).mockResolvedValue({ ...ready, status: "key_missing", problem: "this computer's keychain has no signing key for this publisher identity" });
    const view = renderPage();
    await waitFor(() => expect(view.container.textContent).toContain("This computer has no key for this identity"));
    expect(view.container.textContent).toContain("Restore your key from a backup");
    expect((view.getByRole("button", { name: /Rotate key/ }) as HTMLButtonElement).disabled).toBe(true);
    cleanup();
    vi.mocked(publisherApi.getIdentity).mockResolvedValue({ ...ready, status: "keychain_unavailable", problem: "the system keychain is unavailable: access was denied or the keychain is locked" });
    const locked = renderPage();
    await waitFor(() => expect(locked.container.textContent).toContain("The keychain is unavailable"));
    expect(locked.container.textContent).toContain("keychain is locked");
  });

  it("saves a backup only with a long, repeated passphrase", async () => {
    vi.mocked(publisherApi.exportIdentityBackup).mockResolvedValue(true);
    const view = renderPage();
    const save = await waitFor(() => view.getByRole("button", { name: /Save backup/ }) as HTMLButtonElement);
    fireEvent.change(view.getByLabelText("Backup passphrase"), { target: { value: "short" } });
    expect(view.container.textContent).toContain("Use at least 12 characters.");
    fireEvent.change(view.getByLabelText("Backup passphrase"), { target: { value: "a long passphrase" } });
    fireEvent.change(view.getByLabelText("Repeat backup passphrase"), { target: { value: "a long passphrasf" } });
    expect(view.container.textContent).toContain("The passphrases differ.");
    expect(save.disabled).toBe(true);
    fireEvent.change(view.getByLabelText("Repeat backup passphrase"), { target: { value: "a long passphrase" } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await waitFor(() => expect(view.getByRole("status").textContent).toContain("Backup saved"));
    expect(publisherApi.exportIdentityBackup).toHaveBeenCalledWith("a long passphrase");
    expect((view.getByLabelText("Backup passphrase") as HTMLInputElement).value).toBe("");
  });

  it("restores a backup and reports a wrong passphrase", async () => {
    vi.mocked(publisherApi.getIdentity).mockResolvedValue(none);
    vi.mocked(publisherApi.restoreIdentityBackup)
      .mockRejectedValueOnce({ code: "wrong_passphrase", message: "the passphrase is wrong, or the backup was changed" })
      .mockResolvedValueOnce(ready);
    const view = renderPage();
    const input = await waitFor(() => view.getByLabelText("Restore passphrase"));
    fireEvent.change(input, { target: { value: "guess" } });
    fireEvent.click(view.getByRole("button", { name: /Choose backup/ }));
    await waitFor(() => expect(view.getByRole("alert").textContent).toContain("passphrase is wrong"));
    fireEvent.click(view.getByRole("button", { name: /Choose backup/ }));
    await waitFor(() => expect(view.container.textContent).toContain("Ready to sign"));
  });

  it("rotates after confirmation and toggles signing", async () => {
    vi.mocked(publisherApi.rotateIdentity).mockResolvedValue({ ...ready, rotations: 1 });
    vi.mocked(publisherApi.setSignReleases).mockResolvedValue({ ...ready, signReleases: false });
    const view = renderPage();
    fireEvent.click(await waitFor(() => view.getByRole("button", { name: /Rotate key/ })));
    const dialog = view.getByRole("alertdialog", { name: "Rotate the signing key?" });
    expect(dialog.textContent).toContain("older backups hold the retired key");
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate key" }));
    await waitFor(() => expect(view.container.textContent).toMatch(/Key rotations\s*1/));
    fireEvent.click(view.getByLabelText("Sign new releases"));
    await waitFor(() => expect(view.container.textContent).toContain("New releases will be unsigned"));
    expect(publisherApi.setSignReleases).toHaveBeenCalledWith(false);
  });
});
