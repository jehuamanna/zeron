import { EditorContent, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Code, Heading2, Heading3, Italic, List, ListOrdered, Quote, Redo2, SquareCode, Strikethrough, Undo2 } from "lucide-react";
import { useEffect, type ReactNode } from "react";

export const EMPTY_DOC = JSON.stringify({ type: "doc", content: [{ type: "paragraph" }] });

/**
 * A Tiptap editor for one note. Read-only mode renders a saved note through
 * the same schema, so no stored HTML is ever injected into the page.
 */
export function NoteEditor({ body, editable, label, onChange, autoFocus = false }: {
  body: string;
  editable: boolean;
  label: string;
  onChange?: (json: string, text: string) => void;
  autoFocus?: boolean;
}) {
  const editor = useEditor({
    extensions: [StarterKit.configure({ link: false })],
    content: parse(body),
    editable,
    autofocus: editable && autoFocus ? "end" : false,
    editorProps: { attributes: { "aria-label": label, class: `prose-note text-[15px] text-fg-soft focus:outline-none ${editable ? "min-h-[4.5rem]" : ""}` } },
    onUpdate: ({ editor: current }) => onChange?.(JSON.stringify(current.getJSON()), current.getText()),
  });
  useEffect(() => {
    editor?.setEditable(editable);
  }, [editor, editable]);
  return (
    <div>
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
      strike: current.isActive("strike"),
      code: current.isActive("code"),
      h2: current.isActive("heading", { level: 2 }),
      h3: current.isActive("heading", { level: 3 }),
      bullets: current.isActive("bulletList"),
      numbers: current.isActive("orderedList"),
      quote: current.isActive("blockquote"),
      codeBlock: current.isActive("codeBlock"),
      canUndo: current.can().undo(),
      canRedo: current.can().redo(),
    }),
  });
  const chain = () => editor.chain().focus();
  return (
    <div role="toolbar" aria-label="Formatting" className="mb-2 flex flex-wrap items-center gap-0.5 border-b border-line pb-1.5">
      <Tool label="Bold" active={state.bold} onClick={() => chain().toggleBold().run()}><Bold size={14} /></Tool>
      <Tool label="Italic" active={state.italic} onClick={() => chain().toggleItalic().run()}><Italic size={14} /></Tool>
      <Tool label="Strikethrough" active={state.strike} onClick={() => chain().toggleStrike().run()}><Strikethrough size={14} /></Tool>
      <Tool label="Inline code" active={state.code} onClick={() => chain().toggleCode().run()}><Code size={14} /></Tool>
      <span aria-hidden className="mx-0.5 h-4 w-px bg-line" />
      <Tool label="Heading" active={state.h2} onClick={() => chain().toggleHeading({ level: 2 }).run()}><Heading2 size={14} /></Tool>
      <Tool label="Subheading" active={state.h3} onClick={() => chain().toggleHeading({ level: 3 }).run()}><Heading3 size={14} /></Tool>
      <Tool label="Bulleted list" active={state.bullets} onClick={() => chain().toggleBulletList().run()}><List size={14} /></Tool>
      <Tool label="Numbered list" active={state.numbers} onClick={() => chain().toggleOrderedList().run()}><ListOrdered size={14} /></Tool>
      <Tool label="Quote" active={state.quote} onClick={() => chain().toggleBlockquote().run()}><Quote size={14} /></Tool>
      <Tool label="Code block" active={state.codeBlock} onClick={() => chain().toggleCodeBlock().run()}><SquareCode size={14} /></Tool>
      <span aria-hidden className="mx-0.5 h-4 w-px bg-line" />
      <Tool label="Undo" disabled={!state.canUndo} onClick={() => chain().undo().run()}><Undo2 size={14} /></Tool>
      <Tool label="Redo" disabled={!state.canRedo} onClick={() => chain().redo().run()}><Redo2 size={14} /></Tool>
    </div>
  );
}

function Tool({ label, active, disabled = false, onClick, children }: { label: string; /** Set for toggles only. */ active?: boolean; disabled?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      disabled={disabled}
      // Keep the editor's selection while clicking a tool.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      className={`grid size-7 place-items-center rounded-md transition disabled:opacity-35 ${active ? "bg-accent/15 text-accent-text" : "text-muted hover:bg-white/10 hover:text-fg"}`}
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
