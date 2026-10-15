import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect } from "react";

export const EMPTY_DOC = JSON.stringify({ type: "doc", content: [{ type: "paragraph" }] });

/**
 * A Tiptap editor for one note. Read-only mode renders a saved note through
 * the same schema, so no stored HTML is ever injected into the page.
 */
export function NoteEditor({ body, editable, label, onChange }: {
  body: string;
  editable: boolean;
  label: string;
  onChange?: (json: string, text: string) => void;
}) {
  const editor = useEditor({
    extensions: [StarterKit.configure({ link: false })],
    content: parse(body),
    editable,
    editorProps: { attributes: { "aria-label": label, class: "prose-note min-h-[2.5rem] text-sm text-slate-200 focus:outline-none" } },
    onUpdate: ({ editor: current }) => onChange?.(JSON.stringify(current.getJSON()), current.getText()),
  });
  useEffect(() => {
    editor?.setEditable(editable);
  }, [editor, editable]);
  return <EditorContent editor={editor} />;
}

function parse(body: string): object {
  try {
    return JSON.parse(body) as object;
  } catch {
    return JSON.parse(EMPTY_DOC) as object;
  }
}
