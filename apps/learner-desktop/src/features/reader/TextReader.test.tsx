import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { libraryApi } from "../../services/library";
import { grammarFor } from "./highlight";
import { TextReader } from "./TextReader";

vi.mock("../../services/library", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/library")>()),
  libraryApi: { readText: vi.fn() },
}));

const markdown = [
  "# Consistent hashing",
  "",
  "Read the [paper](https://example.com/paper) first. ![ring](ring.png)",
  "",
  "<script>window.__ran = true</script><img src=x onerror=\"window.__ran = true\">",
  "",
  "| Node | Keys |",
  "| --- | --- |",
  "| a | 3 |",
  "",
  "Use `hash(key)` to place keys.",
  "",
  "```ts",
  "const ring: number[] = [];",
  "```",
  "",
  "```brainfuck",
  "+[----->+++<]>.",
  "```",
].join("\n");

function renderReader() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TextReader courseId="course_1" resourceId="readme" title="Hashing" />
    </QueryClientProvider>,
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("lesson reader", () => {
  it("renders Markdown without HTML, links, or images, and highlights code", async () => {
    vi.mocked(libraryApi.readText).mockResolvedValue({ format: "markdown", text: markdown });
    const view = renderReader();
    const article = await waitFor(() => view.getByRole("article", { name: "Reading Hashing" }));
    expect(libraryApi.readText).toHaveBeenCalledWith("course_1", "readme");
    expect(view.getByRole("heading", { level: 1 }).textContent).toBe("Consistent hashing");
    expect(view.getByRole("table").textContent).toContain("Keys");
    expect(article.querySelector("a, img, script")).toBeNull();
    expect(view.getByText("paper").getAttribute("title")).toBe("https://example.com/paper");
    expect(article.textContent).toContain("[image: ring]");
    expect((window as { __ran?: boolean }).__ran).toBeUndefined();
    expect(view.getByText("hash(key)").tagName).toBe("CODE");

    const typescript = article.querySelector('pre[data-language="ts"]')!;
    await waitFor(() => expect(typescript.querySelectorAll("span[style]").length).toBeGreaterThan(3), { timeout: 10_000 });
    expect(typescript.textContent).toBe("const ring: number[] = [];");
    const unknown = article.querySelector('pre[data-language="brainfuck"]')!;
    expect(unknown.textContent).toBe("+[----->+++<]>.");
    expect(unknown.querySelector("span[style]")).toBeNull();
  });

  it("shows plain text as written and reports unreadable resources", async () => {
    vi.mocked(libraryApi.readText).mockResolvedValueOnce({ format: "text", text: "# not a heading\n  indented" });
    const view = renderReader();
    await waitFor(() => expect(view.getByText(/not a heading/).tagName).toBe("PRE"));
    expect(view.queryByRole("heading")).toBeNull();
    cleanup();

    vi.mocked(libraryApi.readText).mockRejectedValueOnce({ code: "text_unreadable", message: "the resource is not UTF-8 text" });
    const second = renderReader();
    await waitFor(() => expect(second.getByRole("alert").textContent).toBe("the resource is not UTF-8 text"));
  });

  it("maps fence names to bundled grammars", () => {
    expect(["ts", "PY", "c++", "shell", "rust", "brainfuck", undefined].map(grammarFor)).toEqual(["typescript", "python", "cpp", "bash", "rust", null, null]);
  });
});
