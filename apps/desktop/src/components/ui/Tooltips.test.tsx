import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { tooltipText, Tooltips } from "./Tooltips";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("tooltips", () => {
  it("explains icon-only controls, prefers help text, and stays quiet for plain labels", () => {
    const view = render(
      <div>
        <button type="button" aria-label="Rotate pages"><svg /></button>
        <button type="button" title="Download in full and keep offline">Keep offline</button>
        <button type="button">Save note</button>
        <button type="button" aria-label="Save note">Save note</button>
      </div>,
    );
    const [icon, help, plain, same] = view.getAllByRole("button");
    expect(tooltipText(icon!)).toBe("Rotate pages");
    expect(tooltipText(help!)).toBe("Download in full and keep offline");
    expect(tooltipText(plain!)).toBeNull();
    expect(tooltipText(same!)).toBeNull();
  });

  it("shows one themed tooltip on hover and restores the native title afterwards", () => {
    const view = render(
      <>
        <Tooltips />
        <button type="button" title="Fit whole page">Fit</button>
      </>,
    );
    const button = view.getByRole("button");
    fireEvent.pointerOver(button);
    expect(button.hasAttribute("title")).toBe(false);
    act(() => vi.advanceTimersByTime(400));
    expect(document.querySelector("[role='tooltip']")?.textContent).toBe("Fit whole page");
    fireEvent.pointerDown(button);
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    expect(button.getAttribute("title")).toBe("Fit whole page");
  });
});
