import { act, cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { publisherApi, type DraftDetail, type DropSummary, type ResourceSummary, type StructureEdit } from "../../services/publisher";
import { Builder } from "./Builder";

vi.mock("../../services/publisher", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../services/publisher")>()),
  publisherApi: { updateDraftMetadata: vi.fn(), editDraftStructure: vi.fn(), chooseAndAttachResource: vi.fn(), onDrag: vi.fn(), onDrop: vi.fn(), applyDrop: vi.fn(), discardDrop: vi.fn(), chooseLessonSources: vi.fn() },
}));

const initial: DraftDetail = {
  draftId: "draft_one",
  courseId: "course_one",
  courseVersion: "0.1.0",
  title: "Untitled course",
  revision: 0,
  updatedAtUnixSeconds: 1,
  description: null,
  publisherName: "Local publisher",
  language: "en",
  category: null,
  tags: [],
  prerequisites: [],
  licenseName: null,
  modules: [],
};

let server: DraftDetail;
let nextId = 0;

function move<T>(items: T[], from: number, to: number): T[] {
  const copy = [...items];
  const [item] = copy.splice(from, 1);
  copy.splice(to, 0, item!);
  return copy;
}

function apply(draft: DraftDetail, edit: StructureEdit): DraftDetail {
  const modules = draft.modules;
  switch (edit.type) {
    case "addModule":
      return { ...draft, modules: [...modules, { id: `module_${++nextId}`, title: edit.title, lessons: [] }] };
    case "renameModule":
      return { ...draft, modules: modules.map((m) => (m.id === edit.moduleId ? { ...m, title: edit.title } : m)) };
    case "removeModule":
      return { ...draft, modules: modules.filter((m) => m.id !== edit.moduleId) };
    case "moveModule":
      return { ...draft, modules: move(modules, modules.findIndex((m) => m.id === edit.moduleId), edit.position) };
    case "addLesson":
      return { ...draft, modules: modules.map((m) => (m.id === edit.moduleId ? { ...m, lessons: [...m.lessons, { id: `lesson_${++nextId}`, title: edit.title, resources: [] }] } : m)) };
    case "renameLesson":
      return { ...draft, modules: modules.map((m) => ({ ...m, lessons: m.lessons.map((l) => (l.id === edit.lessonId ? { ...l, title: edit.title } : l)) })) };
    case "removeLesson":
      return { ...draft, modules: modules.map((m) => ({ ...m, lessons: m.lessons.filter((l) => l.id !== edit.lessonId) })) };
    case "setLessonPaid":
      return { ...draft, modules: modules.map((m) => ({ ...m, lessons: m.lessons.map((l) => (l.id === edit.lessonId ? { ...l, paid: edit.paid } : l)) })) };
    case "moveLesson":
      return { ...draft, modules: modules.map((m) => (m.id === edit.moduleId ? { ...m, lessons: move(m.lessons, m.lessons.findIndex((l) => l.id === edit.lessonId), edit.position) } : m)) };
    case "removeResource":
      return { ...draft, modules: modules.map((m) => ({ ...m, lessons: m.lessons.map((l) => ({ ...l, resources: l.resources.filter((r) => r.id !== edit.resourceId) })) })) };
    case "moveResource":
      return { ...draft, modules: modules.map((m) => ({ ...m, lessons: m.lessons.map((l) => (l.id === edit.lessonId ? { ...l, resources: move(l.resources, l.resources.findIndex((r) => r.id === edit.resourceId), edit.position) } : l)) })) };
  }
}

function attach(draft: DraftDetail, lessonId: string, resource: ResourceSummary): DraftDetail {
  return { ...draft, modules: draft.modules.map((m) => ({ ...m, lessons: m.lessons.map((l) => (l.id === lessonId ? { ...l, resources: [...l.resources, resource] } : l)) })) };
}

function commit(revision: number, change: (draft: DraftDetail) => DraftDetail): DraftDetail {
  if (revision !== server.revision) throw { code: "revision_conflict", message: "conflict", currentRevision: server.revision };
  server = { ...change(server), revision: server.revision + 1 };
  return server;
}

function Harness({ onReload }: { onReload: () => void }) {
  const [draft, setDraft] = useState(initial);
  return <Builder draft={draft} onSaved={setDraft} onReload={onReload} autosaveDelayMs={5} />;
}

const titles = (view: ReturnType<typeof render>, pattern: RegExp) =>
  view.getAllByLabelText(pattern).map((input) => (input as HTMLInputElement).value);

let dragHandler: (active: boolean) => void = () => undefined;
let dropHandler: (summary: DropSummary) => void = () => undefined;

beforeEach(() => {
  server = initial;
  nextId = 0;
  vi.mocked(publisherApi.onDrag).mockImplementation(async (handler) => {
    dragHandler = handler;
    return () => undefined;
  });
  vi.mocked(publisherApi.onDrop).mockImplementation(async (handler) => {
    dropHandler = handler;
    return () => undefined;
  });
  vi.mocked(publisherApi.discardDrop).mockResolvedValue(undefined);
  vi.mocked(publisherApi.editDraftStructure).mockImplementation(async (_id, revision, edit) => commit(revision, (draft) => apply(draft, edit)));
  vi.mocked(publisherApi.updateDraftMetadata).mockImplementation(async (_id, revision, metadata) => commit(revision, (draft) => ({ ...draft, ...metadata })));
});
afterEach(cleanup);

describe("course structure builder", () => {
  it("adds, renames, and reorders modules and lessons", async () => {
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => expect(view.getAllByLabelText(/^Title of module/)).toHaveLength(1));
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => expect(view.getAllByLabelText(/^Title of module/)).toHaveLength(2));

    const first = view.getByLabelText("Title of module 1");
    fireEvent.focus(first);
    fireEvent.change(first, { target: { value: "Basics" } });
    fireEvent.keyDown(first, { key: "Enter" });
    fireEvent.blur(first);
    await waitFor(() => expect(server.modules[0]!.title).toBe("Basics"));
    expect(vi.mocked(publisherApi.editDraftStructure).mock.calls.filter(([, , edit]) => edit.type === "renameModule")).toHaveLength(1);

    fireEvent.click(view.getByRole("button", { name: "Move module 1 down" }));
    await waitFor(() => expect(titles(view, /^Title of module/)).toEqual(["Untitled module", "Basics"]));
    expect(view.container.textContent).toContain("Moved “Basics” to position 2 of 2.");

    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 2" }));
    await waitFor(() => expect(view.getAllByLabelText(/^Title of lesson/)).toHaveLength(1));
    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 2" }));
    await waitFor(() => expect(view.getAllByLabelText(/^Title of lesson/)).toHaveLength(2));
    const lesson = view.getByLabelText("Title of lesson 1 in module 2");
    fireEvent.focus(lesson);
    fireEvent.change(lesson, { target: { value: "Hello" } });
    fireEvent.keyDown(lesson, { key: "ArrowDown", altKey: true });
    await waitFor(() => expect(titles(view, /^Title of lesson/)).toEqual(["Untitled lesson", "Hello"]));
    expect(view.container.textContent).toContain("Moved “Hello” to position 2 of 2.");
    expect(server.modules[1]!.lessons.map((l) => l.title)).toEqual(["Untitled lesson", "Hello"]);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("marks a lesson paid and back to a free preview", async () => {
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => expect(view.getAllByLabelText(/^Title of module/)).toHaveLength(1));
    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 1" }));
    const toggle = await view.findByRole("button", { name: "Paid: lesson 1 in module 1" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(toggle.textContent).toContain("Free");

    fireEvent.click(toggle);
    await waitFor(() => expect(server.modules[0]!.lessons[0]!.paid).toBe(true));
    await waitFor(() => expect(view.getByRole("button", { name: "Paid: lesson 1 in module 1" }).getAttribute("aria-pressed")).toBe("true"));
    expect(view.container.textContent).toContain("Made lesson 1 in module 1 paid.");

    fireEvent.click(view.getByRole("button", { name: "Paid: lesson 1 in module 1" }));
    await waitFor(() => expect(server.modules[0]!.lessons[0]!.paid).toBe(false));
    expect(view.container.textContent).toContain("Made lesson 1 in module 1 a free preview.");
  });

  it("asks before removing a module that still has lessons", async () => {
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => view.getByRole("button", { name: "Add lesson to module 1" }));
    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 1" }));
    await waitFor(() => view.getByLabelText("Title of lesson 1 in module 1"));

    fireEvent.click(view.getByRole("button", { name: "Remove module 1" }));
    const confirm = view.getByRole("group", { name: "Confirm removing module 1" });
    expect(confirm.textContent).toContain("and its 1 lesson");
    expect(document.activeElement).toBe(within(confirm).getByRole("button", { name: "Cancel" }));
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(view.getByRole("button", { name: "Remove module 1" })));
    expect(server.modules).toHaveLength(1);

    fireEvent.click(view.getByRole("button", { name: "Remove module 1" }));
    fireEvent.click(within(view.getByRole("group", { name: "Confirm removing module 1" })).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(view.queryByLabelText("Title of module 1")).toBeNull());
    expect(server.modules).toHaveLength(0);
  });

  it("removes an empty lesson without confirmation", async () => {
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => view.getByRole("button", { name: "Add lesson to module 1" }));
    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 1" }));
    await waitFor(() => view.getByLabelText("Title of lesson 1 in module 1"));
    fireEvent.click(view.getByRole("button", { name: "Remove lesson 1 in module 1" }));
    await waitFor(() => expect(view.queryByLabelText("Title of lesson 1 in module 1")).toBeNull());
    expect(server.modules[0]!.lessons).toHaveLength(0);
  });

  it("attaches files through the backend picker and shows path-free summaries", async () => {
    const picks: Array<ResourceSummary | null> = [
      { id: "resource_a", kind: "video", fileName: "intro.mp4", mediaType: "video/mp4", byteLength: 2_500_000, durationSeconds: 125 },
      null,
      { id: "resource_b", kind: "subtitle", fileName: "intro.vtt", mediaType: "text/vtt", byteLength: null, durationSeconds: null },
    ];
    vi.mocked(publisherApi.chooseAndAttachResource).mockImplementation(async (_id, revision, lessonId) => {
      const pick = picks.shift();
      return pick ? commit(revision, (draft) => attach(draft, lessonId, pick)) : null;
    });
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => view.getByRole("button", { name: "Add lesson to module 1" }));
    fireEvent.click(view.getByRole("button", { name: "Add lesson to module 1" }));
    const attachButton = await waitFor(() => view.getByRole("button", { name: "Attach file to lesson 1 in module 1" }));

    fireEvent.click(attachButton);
    const resources = await waitFor(() => view.getByRole("list", { name: "Resources of lesson 1 in module 1" }));
    expect(resources.textContent).toContain("intro.mp4");
    expect(resources.textContent).toContain("2.4 MB");
    expect(resources.textContent).toContain("2:05");
    expect(resources.textContent).toContain("Video");
    expect(publisherApi.chooseAndAttachResource).toHaveBeenLastCalledWith("draft_one", 2, "lesson_2");

    fireEvent.click(attachButton);
    await waitFor(() => expect(publisherApi.chooseAndAttachResource).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(view.getByRole("region", { name: "Structure" }).getAttribute("aria-busy")).toBe("false"));
    expect(server.revision).toBe(3);
    expect(view.container.textContent).toContain("Draft revision r3");

    fireEvent.click(attachButton);
    await waitFor(() => expect(view.getByRole("list", { name: "Resources of lesson 1 in module 1" }).textContent).toContain("File missing"));
    expect(publisherApi.chooseAndAttachResource).toHaveBeenLastCalledWith("draft_one", 3, "lesson_2");

    fireEvent.click(view.getByRole("button", { name: "Move resource 2 in lesson 1 in module 1 up" }));
    await waitFor(() => expect(server.modules[0]!.lessons[0]!.resources.map((r) => r.id)).toEqual(["resource_b", "resource_a"]));
    fireEvent.click(view.getByRole("button", { name: "Detach resource 1 in lesson 1 in module 1" }));
    await waitFor(() => expect(server.modules[0]!.lessons[0]!.resources.map((r) => r.id)).toEqual(["resource_a"]));
    expect(view.container.textContent).toContain("The file itself was not changed.");

    fireEvent.click(view.getByRole("button", { name: "Remove lesson 1 in module 1" }));
    expect(view.getByRole("group", { name: "Confirm removing lesson 1 in module 1" }).textContent).toContain("and its 1 resource reference");
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("serializes metadata autosave with structure edits", async () => {
    const view = render(<Harness onReload={vi.fn()} />);
    fireEvent.change(view.getByLabelText("Title"), { target: { value: "Systems" } });
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    await waitFor(() => expect(server.revision).toBe(2));
    await waitFor(() => expect(view.container.textContent).toContain("Draft revision r2"));
    expect(server.title).toBe("Systems");
    expect(server.modules).toHaveLength(1);
    expect(view.queryByRole("alert")).toBeNull();
  });

  it("pauses every editor after a conflict until the publisher reloads", async () => {
    const onReload = vi.fn();
    const view = render(<Harness onReload={onReload} />);
    server = { ...server, title: "Edited elsewhere", revision: 3 };
    fireEvent.click(view.getByRole("button", { name: "Add module" }));
    const alert = await waitFor(() => view.getByRole("alert"));
    expect(alert.textContent).toContain("now r3");
    expect((view.getByLabelText("Title") as HTMLInputElement).disabled).toBe(true);
    expect((view.getByRole("button", { name: "Add module" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(view.getByRole("button", { name: /load latest/i }));
    expect(onReload).toHaveBeenCalledOnce();
    expect(server.modules).toHaveLength(0);
  });
});

const video = (title: string, fileName: string) => ({ title, fileName, kind: "video" as const });

function dropSummary(overrides: Partial<DropSummary> = {}): DropSummary {
  return {
    token: "drop_one",
    x: 40,
    y: 60,
    modules: [{ title: "algoexpert-sdfshortcourse", lessons: [video("lesson1", "lesson1.mp4"), video("lesson2", "lesson2.mp4"), video("lesson10", "lesson10.mp4")] }],
    looseFiles: [{ title: "handout", fileName: "handout.pdf", kind: "pdf" }],
    looseModuleTitle: "Week 1",
    skipped: [{ name: "algoexpert-sdfshortcourse/Design Fundamentals.txt", reason: "unsupported_type" }],
    ...overrides,
  };
}

async function builderWithLesson() {
  const view = render(<Harness onReload={vi.fn()} />);
  fireEvent.click(view.getByRole("button", { name: "Add module" }));
  await waitFor(() => expect(view.getAllByLabelText(/^Title of module/)).toHaveLength(1));
  fireEvent.click(view.getByRole("button", { name: "Add lesson to module 1" }));
  await waitFor(() => expect(view.getAllByLabelText(/^Title of lesson/)).toHaveLength(1));
  await waitFor(() => expect(publisherApi.onDrop).toHaveBeenCalled());
  return view;
}

describe("drag-and-drop import", () => {
  afterEach(() => {
    Reflect.deleteProperty(document, "elementFromPoint");
  });

  it("highlights while dragging and adds folders and files dropped onto a lesson as one revision", async () => {
    const view = await builderWithLesson();
    const lessonId = server.modules[0]!.lessons[0]!.id;
    vi.mocked(publisherApi.applyDrop).mockImplementation(async (_draft, revision, _token, target) =>
      ({
        draft: commit(revision, (draft) => {
          const attached = attach(draft, target!, { id: "res_handout", kind: "pdf", fileName: "handout.pdf", mediaType: "application/pdf", byteLength: 10, durationSeconds: null });
          return { ...attached, modules: [...attached.modules, { id: "module_drop", title: "algoexpert-sdfshortcourse", lessons: [1, 2, 10].map((n) => ({ id: `lesson_drop_${n}`, title: `lesson${n}`, resources: [] })) }] };
        }),
        modulesAdded: 1,
        lessonsAdded: 3,
        filesAttached: 1,
        skipped: [{ name: "algoexpert-sdfshortcourse/Design Fundamentals.txt", reason: "unsupported_type" }],
      }));

    act(() => dragHandler(true));
    expect(view.getByText(/Drop folders to add them as modules/).getAttribute("role")).toBe("status");
    act(() => dragHandler(false));
    expect(view.queryByText(/Drop folders to add them/)).toBeNull();

    const lessonRow = view.container.querySelector(`[data-lesson-id="${lessonId}"]`)!;
    Object.defineProperty(document, "elementFromPoint", { configurable: true, value: vi.fn(() => lessonRow.firstElementChild) });
    act(() => dropHandler(dropSummary()));
    const dialog = view.getByRole("dialog", { name: "Add files and folders" });
    expect(document.elementFromPoint).toHaveBeenCalledWith(40, 60);
    expect(dialog.textContent).toContain("1 folder become 1 new module with 3 lessons");
    expect(within(dialog).getByRole("list", { name: "Modules to add" }).textContent).toContain("lesson10 (lesson10.mp4)");
    expect((within(dialog).getByLabelText(/Add 1 file \(handout.pdf\) as/) as HTMLSelectElement).value).toBe(lessonId);
    expect(within(dialog).getByRole("list", { name: "Left out" }).textContent).toContain("Design Fundamentals.txt — not video, audio, or PDF");

    fireEvent.click(within(dialog).getByRole("button", { name: "Add to course" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(publisherApi.applyDrop).toHaveBeenCalledWith("draft_one", 2, "drop_one", lessonId);
    expect(view.getByText(/Added 1 module with 3 lessons/).textContent).toContain("Added 1 module with 3 lessons and attached 1 file. 1 item left out.");
    await waitFor(() => expect(titles(view, /^Title of module/)).toEqual(["Untitled module", "algoexpert-sdfshortcourse"]));
    expect(view.container.textContent).toContain("Draft revision r3");
  });

  it("adds loose files as a new module unless a lesson is chosen, and discards dismissed or replaced drops", async () => {
    const view = await builderWithLesson();
    act(() => dropHandler(dropSummary({ token: "drop_first", modules: [], skipped: [] })));
    const dialog = view.getByRole("dialog", { name: "Add files and folders" });
    const target = within(dialog).getByLabelText(/Add 1 file/) as HTMLSelectElement;
    expect(target.value).toBe("");
    expect(target.selectedOptions[0]!.textContent).toBe("Lessons of a new module, “Week 1”");
    expect(within(dialog).getByRole("button", { name: "Add to course" }).hasAttribute("disabled")).toBe(false);
    fireEvent.change(target, { target: { value: server.modules[0]!.lessons[0]!.id } });
    expect(target.selectedOptions[0]!.textContent).toContain("Attachments of");

    act(() => dropHandler(dropSummary({ token: "drop_second" })));
    expect(publisherApi.discardDrop).toHaveBeenCalledWith("drop_first");
    fireEvent.click(within(view.getByRole("dialog")).getByRole("button", { name: "Dismiss" }));
    expect(publisherApi.discardDrop).toHaveBeenCalledWith("drop_second");
    expect(view.queryByRole("dialog")).toBeNull();
    expect(publisherApi.applyDrop).not.toHaveBeenCalled();
  });

  it("picks folders or files in bulk and adds them like a drop", async () => {
    const view = await builderWithLesson();
    vi.mocked(publisherApi.chooseLessonSources)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(dropSummary({ token: "pick_one", looseFiles: [] }))
      .mockResolvedValueOnce(dropSummary({ token: "pick_two", modules: [], skipped: [] }));
    vi.mocked(publisherApi.applyDrop).mockImplementation(async (_draft, revision) => ({
      draft: commit(revision, (draft) => ({ ...draft, modules: [...draft.modules, { id: "module_week", title: "Week 1", lessons: [{ id: "lesson_handout", title: "handout", resources: [] }] }] })),
      modulesAdded: 1,
      lessonsAdded: 1,
      filesAttached: 0,
      skipped: [],
    }));

    fireEvent.click(view.getByRole("button", { name: /Add folders/ }));
    await waitFor(() => expect(publisherApi.chooseLessonSources).toHaveBeenCalledWith("folders"));
    expect(view.queryByRole("dialog")).toBeNull();

    fireEvent.click(view.getByRole("button", { name: /Add folders/ }));
    const folders = await waitFor(() => view.getByRole("dialog", { name: "Add files and folders" }));
    expect(folders.textContent).toContain("1 folder become 1 new module with 3 lessons");
    fireEvent.click(within(folders).getByRole("button", { name: "Dismiss" }));
    expect(publisherApi.discardDrop).toHaveBeenCalledWith("pick_one");

    fireEvent.click(view.getByRole("button", { name: /Add files/ }));
    await waitFor(() => expect(publisherApi.chooseLessonSources).toHaveBeenLastCalledWith("files"));
    const files = await waitFor(() => view.getByRole("dialog", { name: "Add files and folders" }));
    fireEvent.click(within(files).getByRole("button", { name: "Add to course" }));
    await waitFor(() => expect(view.queryByRole("dialog")).toBeNull());
    expect(publisherApi.applyDrop).toHaveBeenCalledWith("draft_one", 2, "pick_two", null);
    await waitFor(() => expect(titles(view, /^Title of module/)).toEqual(["Untitled module", "Week 1"]));
  });

  it("explains expired drops and pauses on conflicts like any other edit", async () => {
    const view = await builderWithLesson();
    vi.mocked(publisherApi.applyDrop)
      .mockRejectedValueOnce({ code: "drop_expired", message: "this drop expired; drop the files again" })
      .mockRejectedValueOnce({ code: "revision_conflict", message: "conflict", currentRevision: 9 });
    act(() => dropHandler(dropSummary({ looseFiles: [] })));
    fireEvent.click(view.getByRole("button", { name: "Add to course" }));
    await waitFor(() => expect(within(view.getByRole("dialog")).getByRole("alert").textContent).toContain("drop the files again"));
    fireEvent.click(view.getByRole("button", { name: "Add to course" }));
    await waitFor(() => expect(view.container.textContent).toContain("This draft was changed elsewhere (now r9)"));
    expect(view.getByRole("button", { name: "Add to course" }).hasAttribute("disabled")).toBe(true);
  });

  it("says when nothing in a drop can be added", async () => {
    const view = await builderWithLesson();
    act(() => dropHandler(dropSummary({ modules: [], looseFiles: [], skipped: [{ name: "link.mp4", reason: "symbolic_link" }] })));
    const dialog = view.getByRole("dialog");
    expect(dialog.textContent).toContain("Nothing here can be added.");
    expect(dialog.textContent).toContain("link, not followed");
    expect(within(dialog).getByRole("button", { name: "Add to course" }).hasAttribute("disabled")).toBe(true);
  });
});
