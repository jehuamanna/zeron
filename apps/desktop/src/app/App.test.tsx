import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useShell } from "../stores/shell";
import { App } from "./App";

vi.mock("./Library", () => ({ Library: () => <p>library area</p> }));
vi.mock("../publish/PublishArea", () => ({
  PublishArea: ({ onOpenNetwork }: { onOpenNetwork: () => void }) => (
    <div>
      <p>publish area</p>
      <button type="button" onClick={onOpenNetwork}>Network settings</button>
    </div>
  ),
}));
vi.mock("../features/network/NetworkSettings", () => ({
  NetworkSettings: ({ onBack }: { onBack: () => void }) => (
    <div>
      <p>network settings</p>
      <button type="button" onClick={onBack}>Back</button>
    </div>
  ),
}));

beforeEach(() => useShell.setState({ area: "library", networkOpen: false }));
afterEach(cleanup);

describe("Zeron shell", () => {
  it("shows Library and Publish side by side and switches between them", () => {
    const view = render(<App />);
    const nav = view.getByRole("navigation", { name: "Zeron" });
    const library = view.getByRole("button", { name: "Library" });
    const publish = view.getByRole("button", { name: "Publish" });
    expect(nav.contains(library) && nav.contains(publish)).toBe(true);
    expect(library.getAttribute("aria-current")).toBe("page");
    expect(view.container.textContent).toContain("library area");

    fireEvent.click(publish);
    expect(view.container.textContent).toContain("publish area");
    expect(view.container.textContent).not.toContain("library area");
    expect(publish.getAttribute("aria-current")).toBe("page");
    expect(library.getAttribute("aria-current")).toBeNull();

    fireEvent.click(library);
    expect(view.container.textContent).toContain("library area");
  });

  it("opens the one Network setting from the header or from Publish and returns", () => {
    const view = render(<App />);
    fireEvent.click(view.getByRole("button", { name: "Network" }));
    expect(view.container.textContent).toContain("network settings");
    expect(view.getByRole("button", { name: "Library" }).getAttribute("aria-current")).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "Back" }));
    expect(view.container.textContent).toContain("library area");

    fireEvent.click(view.getByRole("button", { name: "Publish" }));
    fireEvent.click(view.getByRole("button", { name: "Network settings" }));
    expect(view.container.textContent).toContain("network settings");
    fireEvent.click(view.getByRole("button", { name: "Back" }));
    expect(view.container.textContent).toContain("publish area");

    fireEvent.click(view.getByRole("button", { name: "Network" }));
    fireEvent.click(view.getByRole("button", { name: "Library" }));
    expect(view.container.textContent).toContain("library area");
  });
});
