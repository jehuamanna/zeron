import Highlight from "@tiptap/extension-highlight";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { EditorContent, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Code, Highlighter, Italic, Link2, List, ListChecks, ListOrdered, Minus, Quote, Redo2, SquareCode, Strikethrough, Underline, Undo2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

export const EMPTY_DOC = JSON.stringify({ type: "doc", content: [{ type: "paragraph" }] });

/** Links are kept for reference; the app never opens them, as lesson readings never do. */
const isAllowedUri = (url: string) => /^(https?:\/\/|mailto:)/i.test(url.trim());

const extensions = [
  StarterKit.configure({
    link: { openOnClick: false, autolink: true, isAllowedUri, HTMLAttributes: { rel: "noopener noreferrer nofollow", target: null } },
  }),
  Highlight.configure({ HTMLAttributes: { class: "note-mark" } }),
  TaskList,
  TaskItem.configure({ nested: true }),
];

/**
 * A Tiptap editor for one note. Read-only mode renders a saved note through
 * the same schema, so no stored HTML is ever injected into the page.
 */
export function NoteEditor({ body, editable, label, onChange, autoFocus = false, fill = false }: {
  body: string;
  editable: boolean;
  label: string;
  onChange?: (json: string, text: string) => void;
  autoFocus?: boolean;
  /** Fill the parent's height: the toolbar stays, the text scrolls. */
  fill?: boolean;
}) {
  const editor = useEditor({
    extensions,
    content: parse(body),
    editable,
    autofocus: editable && autoFocus ? "end" : false,
    editorProps: { attributes: { "aria-label": label, class: `prose-note text-[15px] text-fg-soft focus:outline-none ${fill ? "min-h-full" : editable ? "min-h-[4.5rem]" : ""}` } },
    onUpdate: ({ editor: current }) => onChange?.(JSON.stringify(current.getJSON()), current.getText()),
  });
  useEffect(() => {
    editor?.setEditable(editable);
  }, [editor, editable]);
  // Links never navigate the app window.
  const stopLinks = (event: React.MouseEvent) => {
    if ((event.target as Element).closest("a")) event.preventDefault();
  };
  if (fill) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        {editable && editor ? <Toolbar editor={editor} /> : null}
        <EditorContent
          editor={editor}
          className="min-h-0 flex-1 cursor-text overflow-y-auto [&>.tiptap]:h-full"
          onClick={(event) => {
            stopLinks(event);
            editor?.commands.focus();
          }}
        />
      </div>
    );
  }
  return (
    <div onClickCapture={stopLinks}>
      {editable && editor ? <Toolbar editor={editor} /> : null}
      <EditorContent editor={editor} />
    </div>
  );
}

function Toolbar({ editor }: { editor: Editor }) {
  const state = useEditorState({
    editor,
    selector: ({ editor: current }) => ({
      bold: current.isActive("bold"),
      italic: current.isActive("italic"),
      underline: current.isActive("underline"),
      strike: current.isActive("strike"),
      highlight: current.isActive("highlight"),
      code: current.isActive("code"),
      link: current.isActive("link"),
      href: (current.getAttributes("link").href as string | undefined) ?? "",
      h1: current.isActive("heading", { level: 1 }),
      h2: current.isActive("heading", { level: 2 }),
      h3: current.isActive("heading", { level: 3 }),
      bullets: current.isActive("bulletList"),
      numbers: current.isActive("orderedList"),
      tasks: current.isActive("taskList"),
      quote: current.isActive("blockquote"),
      codeBlock: current.isActive("codeBlock"),
      canUndo: current.can().undo(),
      canRedo: current.can().redo(),
    }),
  });
  const [linking, setLinking] = useState(false);
  const [url, setUrl] = useState("");
  const chain = () => editor.chain().focus();
  const applyLink = () => {
    const value = url.trim();
    if (!value) chain().extendMarkRange("link").unsetLink().run();
    else if (isAllowedUri(value)) chain().extendMarkRange("link").setLink({ href: value }).run();
    setLinking(false);
  };
  const divider = <span aria-hidden className="mx-1 h-4 w-px bg-line" />;

  return (
    <div className="mb-2 border-b border-line pb-1.5">
      <div role="toolbar" aria-label="Formatting" className="flex flex-wrap items-center gap-0.5">
        <Tool label="Undo" shortcut="Ctrl+Z" disabled={!state.canUndo} onClick={() => chain().undo().run()}><Undo2 size={15} /></Tool>
        <Tool label="Redo" shortcut="Ctrl+Shift+Z" disabled={!state.canRedo} onClick={() => chain().redo().run()}><Redo2 size={15} /></Tool>
        {divider}
        <Tool label="Heading 1" active={state.h1} onClick={() => chain().toggleHeading({ level: 1 }).run()}><Level n={1} /></Tool>
        <Tool label="Heading 2" active={state.h2} onClick={() => chain().toggleHeading({ level: 2 }).run()}><Level n={2} /></Tool>
        <Tool label="Heading 3" active={state.h3} onClick={() => chain().toggleHeading({ level: 3 }).run()}><Level n={3} /></Tool>
        {divider}
        <Tool label="Bold" shortcut="Ctrl+B" active={state.bold} onClick={() => chain().toggleBold().run()}><Bold size={15} /></Tool>
        <Tool label="Italic" shortcut="Ctrl+I" active={state.italic} onClick={() => chain().toggleItalic().run()}><Italic size={15} /></Tool>
        <Tool label="Underline" shortcut="Ctrl+U" active={state.underline} onClick={() => chain().toggleUnderline().run()}><Underline size={15} /></Tool>
        <Tool label="Strikethrough" active={state.strike} onClick={() => chain().toggleStrike().run()}><Strikethrough size={15} /></Tool>
        <Tool label="Highlight" active={state.highlight} onClick={() => chain().toggleHighlight().run()}><Highlighter size={15} /></Tool>
        <Tool label="Inline code" active={state.code} onClick={() => chain().toggleCode().run()}><Code size={15} /></Tool>
        <Tool
          label="Link"
          active={state.link}
          onClick={() => {
            setUrl(state.href);
            setLinking(!linking);
          }}
        >
          <Link2 size={15} />
        </Tool>
        {divider}
        <Tool label="Bulleted list" active={state.bullets} onClick={() => chain().toggleBulletList().run()}><List size={15} /></Tool>
        <Tool label="Numbered list" active={state.numbers} onClick={() => chain().toggleOrderedList().run()}><ListOrdered size={15} /></Tool>
        <Tool label="Checklist" active={state.tasks} onClick={() => chain().toggleTaskList().run()}><ListChecks size={15} /></Tool>
        <Tool label="Quote" active={state.quote} onClick={() => chain().toggleBlockquote().run()}><Quote size={15} /></Tool>
        <Tool label="Code block" active={state.codeBlock} onClick={() => chain().toggleCodeBlock().run()}><SquareCode size={15} /></Tool>
        <Tool label="Divider" onClick={() => chain().setHorizontalRule().run()}><Minus size={15} /></Tool>
      </div>
      {linking ? (
        <form
          className="mt-1.5 flex items-center gap-1.5"
          onSubmit={(event) => {
            event.preventDefault();
            applyLink();
          }}
        >
          <input
            autoFocus
            type="url"
            aria-label="Link address"
            placeholder="https://"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation();
                setLinking(false);
              }
            }}
            className="min-w-0 flex-1 rounded-md border border-line bg-surface px-2 py-1 text-xs text-fg"
          />
          <button type="submit" className="rounded-md bg-accent px-2 py-1 text-xs font-medium text-on-accent disabled:opacity-50" disabled={url.trim() !== "" && !isAllowedUri(url)}>
            {url.trim() ? "Apply" : "Remove"}
          </button>
        </form>
      ) : null}
    </div>
  );
}

function Level({ n }: { n: 1 | 2 | 3 }) {
  return (
    <span className="text-[13px] font-semibold leading-none">
      H<sub className="text-[9px]">{n}</sub>
    </span>
  );
}

function Tool({ label, shortcut, active, disabled = false, onClick, children }: {
  label: string;
  shortcut?: string;
  /** Set for toggles only. */
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={shortcut ? `${label} (${shortcut})` : label}
      aria-pressed={active}
      disabled={disabled}
      // Keep the editor's selection while clicking a tool.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      className={`grid size-8 place-items-center rounded-md transition disabled:opacity-35 ${active ? "bg-accent/15 text-accent-text" : "text-muted hover:bg-white/10 hover:text-fg"}`}
    >
      {children}
    </button>
  );
}

function parse(body: string): object {
  try {
    return JSON.parse(body) as object;
  } catch {
    return JSON.parse(EMPTY_DOC) as object;
  }
}
