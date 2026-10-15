import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Clock, NotebookPen, Pencil, Trash2 } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type Note, type NoteAnchor } from "../../services/library";
import { EMPTY_DOC, NoteEditor } from "./NoteEditor";

export function formatTimestamp(seconds: number) {
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, "0");
  return hours ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

function anchorLabel(anchor: NoteAnchor) {
  switch (anchor.kind) {
    case "lesson":
      return "Lesson";
    case "videoTime":
      return formatTimestamp(anchor.seconds);
    case "pdfPage":
      return anchor.region ? `Page ${anchor.page}, highlight` : `Page ${anchor.page}`;
  }
}

type Draft = { noteId: string | null; revision: number; anchor: NoteAnchor; body: string; text: string };

/** Notes of one lesson. The player handle supplies timestamps and seeking. */
export function NotesPanel({ courseId, lessonId, lessonTitle, videoResourceId, currentTime, onSeek, onOpenPage, documentAnchor = null, onNoteCreated }: {
  courseId: string;
  lessonId: string;
  lessonTitle: string;
  /** The lesson's video being played, if any, for timestamped notes. */
  videoResourceId: string | null;
  currentTime: () => number;
  onSeek: (resourceId: string, seconds: number) => void;
  onOpenPage?: (resourceId: string, page: number) => void;
  /** The open PDF page, and any highlighted area, for page notes. */
  documentAnchor?: Extract<NoteAnchor, { kind: "pdfPage" }> | null;
  onNoteCreated?: () => void;
}) {
  const queryClient = useQueryClient();
  const key = ["notes", courseId, lessonId];
  const notes = useQuery({ queryKey: key, queryFn: () => libraryApi.notes(courseId, lessonId) });
  const [draft, setDraft] = useState<Draft | null>(null);
  const [conflict, setConflict] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: key });

  const save = useMutation({
    mutationFn: async (current: Draft) => {
      if (current.noteId === null) {
        await libraryApi.createNote(courseId, lessonId, current.anchor, current.body);
        return "saved" as const;
      }
      const change = await libraryApi.updateNote(current.noteId, current.revision, current.anchor, current.body);
      return change.outcome;
    },
    onSuccess: (outcome, saved) => {
      if (saved.noteId === null) onNoteCreated?.();
      if (outcome === "conflict") {
        // Keep the learner's text on screen; nothing was overwritten.
        setConflict(true);
        return;
      }
      setDraft(null);
      setConflict(false);
      void refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (note: Note) => libraryApi.deleteNote(note.noteId, note.revision),
    onSuccess: () => void refresh(),
  });

  const start = (anchor: NoteAnchor) => {
    setConflict(false);
    setDraft({ noteId: null, revision: 0, anchor, body: EMPTY_DOC, text: "" });
  };
  const error = save.isError ? (asCommandError(save.error)?.message ?? "The note could not be saved.") : remove.isError ? (asCommandError(remove.error)?.message ?? "The note could not be deleted.") : null;

  return (
    <section aria-label={`Notes for ${lessonTitle}`} className="mt-6 rounded-3xl border border-white/10 bg-black/20 p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="flex items-center gap-2 font-semibold"><NotebookPen size={17} className="text-cyan-300" /> Notes · {lessonTitle}</h2>
        <div className="flex flex-wrap gap-2">
          {videoResourceId ? (
            <Button variant="ghost" disabled={draft !== null} onClick={() => start({ kind: "videoTime", resourceId: videoResourceId, seconds: Math.floor(currentTime()) })}>
              <Clock size={15} /> Add note at {formatTimestamp(currentTime())}
            </Button>
          ) : null}
          {documentAnchor ? (
            <Button variant="ghost" disabled={draft !== null} onClick={() => start(documentAnchor)}>
              {documentAnchor.region ? "Add note on highlight" : `Add note on page ${documentAnchor.page}`}
            </Button>
          ) : null}
          <Button variant="ghost" disabled={draft !== null} onClick={() => start({ kind: "lesson" })}>Add lesson note</Button>
        </div>
      </div>

      {draft ? (
        <div className="mt-4 rounded-2xl border border-cyan-300/30 bg-black/30 p-4">
          <p className="mb-2 text-xs text-cyan-200">{draft.noteId ? "Editing note" : "New note"} · {anchorLabel(draft.anchor)}</p>
          <NoteEditor key={draft.noteId ?? "new"} body={draft.body} editable label="Note text" onChange={(body, text) => setDraft({ ...draft, body, text })} />
          {conflict ? (
            <p role="alert" className="mt-2 text-xs text-amber-200">
              This note changed since you opened it, so your edit was not saved. Your text is still here; copy what you need, then load the latest version.
            </p>
          ) : null}
          <div className="mt-3 flex gap-2">
            <Button disabled={save.isPending || conflict || draft.text.trim() === ""} onClick={() => save.mutate(draft)}>{save.isPending ? "Saving…" : "Save note"}</Button>
            {conflict ? (
              <Button variant="ghost" onClick={() => { setDraft(null); setConflict(false); void refresh(); }}>Load latest</Button>
            ) : (
              <Button variant="ghost" disabled={save.isPending} onClick={() => setDraft(null)}>Cancel</Button>
            )}
          </div>
        </div>
      ) : null}
      {error ? <p role="alert" className="mt-3 text-xs text-red-300">{error}</p> : null}

      {notes.isPending ? <p className="mt-4 text-sm text-slate-400">Loading notes…</p> : notes.data?.length ? (
        <ul aria-label="Notes" className="mt-4 space-y-3">
          {notes.data.map((note) => (
            <li key={note.noteId} className="rounded-2xl border border-white/10 bg-white/[0.03] p-4">
              <div className="mb-2 flex items-center justify-between gap-2">
                {note.anchor.kind === "lesson" ? (
                  <span className="rounded-md bg-white/5 px-2 py-0.5 text-xs text-slate-400">Lesson</span>
                ) : (
                  <button
                    type="button"
                    aria-label={`Go to ${anchorLabel(note.anchor)}`}
                    onClick={() => {
                      const anchor = note.anchor;
                      if (anchor.kind === "videoTime") onSeek(anchor.resourceId, anchor.seconds);
                      if (anchor.kind === "pdfPage") onOpenPage?.(anchor.resourceId, anchor.page);
                    }}
                    className="rounded-md bg-cyan-300/10 px-2 py-0.5 text-xs text-cyan-200 hover:bg-cyan-300/20"
                  >
                    {anchorLabel(note.anchor)}
                  </button>
                )}
                <div className="flex gap-1">
                  <button type="button" aria-label="Edit note" disabled={draft !== null} onClick={() => { setConflict(false); setDraft({ noteId: note.noteId, revision: note.revision, anchor: note.anchor, body: note.body, text: note.text }); }} className="rounded-md p-1 text-slate-400 hover:text-slate-100 disabled:opacity-40"><Pencil size={14} /></button>
                  <button type="button" aria-label="Delete note" disabled={remove.isPending} onClick={() => remove.mutate(note)} className="rounded-md p-1 text-slate-400 hover:text-red-300 disabled:opacity-40"><Trash2 size={14} /></button>
                </div>
              </div>
              <NoteEditor key={`${note.noteId}:${note.revision}`} body={note.body} editable={false} label="Note" />
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-4 text-sm text-slate-500">No notes for this lesson yet.</p>
      )}
    </section>
  );
}
