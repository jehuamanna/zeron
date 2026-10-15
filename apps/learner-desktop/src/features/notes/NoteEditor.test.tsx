import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { NoteEditor } from "./NoteEditor";

afterEach(cleanup);

describe("note editor", () => {
  it("renders a stored Tiptap document read-only without injecting markup", async () => {
    const body = JSON.stringify({
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "Load " }, { type: "text", marks: [{ type: "bold" }], text: "balancers" }] },
        { type: "paragraph", content: [{ type: "text", text: "<img src=x onerror=alert(1)>" }] },
      ],
    });
    const view = render(<NoteEditor body={body} editable={false} label="Note" />);
    const editor = await waitFor(() => view.getByLabelText("Note"));
    expect(editor.getAttribute("contenteditable")).toBe("false");
    expect(editor.textContent).toContain("Load balancers");
    expect(editor.querySelector("strong")?.textContent).toBe("balancers");
    expect(editor.querySelector("img")).toBeNull();
    expect(editor.textContent).toContain("<img src=x onerror=alert(1)>");
  });

  it("falls back to an empty document for unreadable content", async () => {
    const view = render(<NoteEditor body="not json" editable label="Note text" />);
    const editor = await waitFor(() => view.getByLabelText("Note text"));
    expect(editor.getAttribute("contenteditable")).toBe("true");
    expect(editor.textContent).toBe("");
  });
});
