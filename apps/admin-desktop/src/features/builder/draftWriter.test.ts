import { describe, expect, it, vi } from "vitest";

import type { DraftDetail } from "../../services/publisher";
import { createDraftWriter } from "./draftWriter";

const detail = (revision: number) => ({ revision }) as DraftDetail;

describe("draft writer", () => {
  it("serializes writes and hands each the latest revision", async () => {
    const onSaved = vi.fn();
    const writer = createDraftWriter(3, { onSaved, onConflict: vi.fn() });
    const seen: number[] = [];
    let releaseFirst!: () => void;
    const first = writer.submit((revision) => {
      seen.push(revision);
      return new Promise((resolve) => (releaseFirst = () => resolve(detail(revision + 1))));
    });
    const second = writer.submit(async (revision) => {
      seen.push(revision);
      return detail(revision + 1);
    });
    await Promise.resolve();
    expect(seen).toEqual([3]);
    releaseFirst();
    await expect(second).resolves.toEqual(detail(5));
    await expect(first).resolves.toEqual(detail(4));
    expect(seen).toEqual([3, 4]);
    expect(writer.revision).toBe(5);
    expect(onSaved).toHaveBeenCalledTimes(2);
  });

  it("refuses every write after a conflict", async () => {
    const onConflict = vi.fn();
    const writer = createDraftWriter(1, { onSaved: vi.fn(), onConflict });
    await expect(writer.submit(() => Promise.reject({ code: "revision_conflict", message: "c", currentRevision: 2 }))).rejects.toMatchObject({ code: "revision_conflict" });
    expect(onConflict).toHaveBeenCalledWith(2);
    const later = vi.fn();
    await expect(writer.submit(later)).rejects.toMatchObject({ code: "revision_conflict" });
    expect(later).not.toHaveBeenCalled();
    expect(onConflict).toHaveBeenCalledOnce();
  });

  it("keeps writing after an ordinary failure", async () => {
    const writer = createDraftWriter(1, { onSaved: vi.fn(), onConflict: vi.fn() });
    await expect(writer.submit(() => Promise.reject({ code: "invalid_input", message: "bad" }))).rejects.toMatchObject({ code: "invalid_input" });
    await expect(writer.submit(async (revision) => detail(revision + 1))).resolves.toEqual(detail(2));
  });
});
