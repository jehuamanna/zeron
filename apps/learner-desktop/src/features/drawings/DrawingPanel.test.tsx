import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../../services/library";
import type { SceneChange } from "./DrawingCanvas";
import { DrawingPanel } from "./DrawingPanel";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { drawing: vi.fn(), saveDrawing: vi.fn() },
}));

const scene = (ids: string[]) => JSON.stringify({ elements: ids.map((id) => ({ id })), appState: { viewBackgroundColor: "#fff" } });
const image = { id: "img1", mimeType: "image/png", dataUrl: "data:image/png;base64,AAAA" };

// Excalidraw needs a real canvas; the stand-in reports scene changes from buttons.
vi.mock("./DrawingCanvas", () => ({
  default: ({ sceneJson, onChange }: { sceneJson: string | null; onChange: (change: SceneChange) => void }) => (
    <div>
      <p data-testid="loaded">{sceneJson ?? "empty"}</p>
      <button type="button" onClick={() => onChange({ sceneJson: scene([]), files: [] })}>blank</button>
      <button type="button" onClick={() => onChange({ sceneJson: scene(["a"]), files: [image] })}>draw a</button>
      <button type="button" onClick={() => onChange({ sceneJson: scene(["a", "b"]), files: [image] })}>draw b</button>
    </div>
  ),
}));

function renderPanel() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <DrawingPanel courseId="course_1" lessonId="lesson_1" lessonTitle="Caching" />
    </QueryClientProvider>,
  );
}

async function openDrawing(view: ReturnType<typeof renderPanel>) {
  fireEvent.click(view.getByRole("button", { name: "Open drawing" }));
  await waitFor(() => view.getByTestId("loaded"));
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("lesson drawings", () => {
  it("saves a new drawing once drawing pauses, then continues from its revision", async () => {
    vi.mocked(libraryApi.drawing).mockResolvedValue(null);
    vi.mocked(libraryApi.saveDrawing).mockResolvedValueOnce({ outcome: "saved", revision: 0 }).mockResolvedValueOnce({ outcome: "saved", revision: 1 });
    const view = renderPanel();
    expect(libraryApi.drawing).not.toHaveBeenCalled();
    await openDrawing(view);
    expect(libraryApi.drawing).toHaveBeenCalledWith("course_1", "lesson_1");

    fireEvent.click(view.getByRole("button", { name: "blank" }));
    fireEvent.click(view.getByRole("button", { name: "draw a" }));
    await waitFor(() => expect(view.getByText("Saved")).toBeTruthy(), { timeout: 3000 });
    expect(libraryApi.saveDrawing).toHaveBeenCalledTimes(1);
    expect(libraryApi.saveDrawing).toHaveBeenCalledWith("course_1", "lesson_1", null, scene(["a"]), [image]);

    fireEvent.click(view.getByRole("button", { name: "draw b" }));
    await waitFor(() => expect(libraryApi.saveDrawing).toHaveBeenCalledTimes(2), { timeout: 3000 });
    expect(libraryApi.saveDrawing).toHaveBeenLastCalledWith("course_1", "lesson_1", 0, scene(["a", "b"]), []);
  });

  it("opens the saved drawing and never overwrites a newer one", async () => {
    vi.mocked(libraryApi.drawing).mockResolvedValue({ sceneJson: scene(["a"]), revision: 3, files: [image] });
    vi.mocked(libraryApi.saveDrawing).mockResolvedValue({ outcome: "conflict", currentRevision: 4 });
    const view = renderPanel();
    await openDrawing(view);
    expect(view.getByTestId("loaded").textContent).toBe(scene(["a"]));

    // The editor first reports the restored scene, reserialized; that is not a change.
    fireEvent.click(view.getByRole("button", { name: "draw a" }));
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(libraryApi.saveDrawing).not.toHaveBeenCalled();
    fireEvent.click(view.getByRole("button", { name: "draw b" }));
    const alert = await waitFor(() => view.getByRole("alert"), { timeout: 3000 });
    expect(alert.textContent).toContain("changed in another window");
    expect(libraryApi.saveDrawing).toHaveBeenCalledWith("course_1", "lesson_1", 3, scene(["a", "b"]), []);

    fireEvent.click(view.getByRole("button", { name: "draw a" }));
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(libraryApi.saveDrawing).toHaveBeenCalledTimes(1);

    vi.mocked(libraryApi.drawing).mockResolvedValue({ sceneJson: scene(["z"]), revision: 4, files: [] });
    fireEvent.click(view.getByRole("button", { name: "Load the saved drawing" }));
    await waitFor(() => expect(view.getByTestId("loaded").textContent).toBe(scene(["z"])));
    expect(view.queryByRole("alert")).toBeNull();
  });
});
