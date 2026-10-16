import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookmarkPlus, Clock, FileText, NotebookPen, Pencil, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

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

export function anchorLabel(anchor: NoteAnchor) {
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

/** A request from outside the panel (the player, a shortcut) to start a note. */
export type NoteRequest = { id: number; anchor: NoteAnchor };

/** Notes of one lesson. The player handle supplies timestamps and seeking. */
export function NotesPanel({ courseId, lessonId, lessonTitle, videoResourceId, currentTime, onSeek, onOpenPage, documentAnchor = null, onNoteCreated, request = null, onRequestHandled }: {
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
  request?: NoteRequest | null;
  /** Called once a request has started a note, so it is not replayed. */
  onRequestHandled?: () => void;
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

  // Follow the player so "Add note at" shows the current moment.
  const [, tick] = useState(0);
  useEffect(() => {
    if (!videoResourceId) return;
    const timer = setInterval(() => tick((count) => count + 1), 1000);
    return () => clearInterval(timer);
  }, [videoResourceId]);

  useEffect(() => {
    if (!request) return;
    if (draft === null) start(request.anchor);
    onRequestHandled?.();
    // A new request id starts a note; an open draft is never replaced.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request?.id]);
  const error = save.isError ? (asCommandError(save.error)?.message ?? "The note could not be saved.") : remove.isError ? (asCommandError(remove.error)?.message ?? "The note could not be deleted.") : null;

  const canSave = !save.isPending && !conflict && draft !== null && draft.text.trim() !== "";
  const action = "inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium text-muted hover:bg-white/10 hover:text-fg disabled:opacity-40";

  return (
    <section aria-label={`Notes for ${lessonTitle}`} className="flex h-full min-h-0 flex-col">
      {draft ? (
        <div
          className="flex min-h-0 flex-1 flex-col"
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && canSave) {
              event.preventDefault();
              save.mutate(draft);
            }
            if (event.key === "Escape" && !save.isPending && !conflict) setDraft(null);
          }}
        >
          <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
            <p className="flex min-w-0 flex-1 items-center gap-1.5 truncate text-xs font-medium text-accent-text">
              {draft.anchor.kind === "videoTime" ? <Clock size={12} /> : draft.anchor.kind === "pdfPage" ? <FileText size={12} /> : <BookmarkPlus size={12} />}
              {draft.noteId ? "Editing note" : "New note"} · {anchorLabel(draft.anchor)}
            </p>
            {conflict ? (
              <Button size="sm" variant="ghost" onClick={() => { setDraft(null); setConflict(false); void refresh(); }}>Load latest</Button>
            ) : (
              <Button size="sm" variant="quiet" disabled={save.isPending} onClick={() => setDraft(null)}>Cancel</Button>
            )}
            <Button size="sm" disabled={!canSave} title="Save (Ctrl+Enter)" onClick={() => save.mutate(draft)}>{save.isPending ? "Saving…" : "Save note"}</Button>
          </div>
          {conflict ? (
            <p role="alert" className="shrink-0 px-3 pt-2 text-xs text-amber-200">
              This note changed since you opened it, so your edit was not saved. Your text is still here; copy what you need, then load the latest version.
            </p>
          ) : null}
          {error ? <p role="alert" className="shrink-0 px-3 pt-2 text-xs text-red-300">{error}</p> : null}
          <div className="min-h-0 flex-1 px-3 py-2">
            <NoteEditor key={draft.noteId ?? "new"} fill body={draft.body} editable autoFocus label="Note text" onChange={(body, text) => setDraft({ ...draft, body, text })} />
          </div>
        </div>
      ) : (
      <>
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-line px-3 py-2">
        {videoResourceId ? (
          <button type="button" className={`${action} text-accent-text`} onClick={() => start({ kind: "videoTime", resourceId: videoResourceId, seconds: Math.floor(currentTime()) })}>
            <Clock size={13} /> Add note at {formatTimestamp(currentTime())}
          </button>
        ) : null}
        {documentAnchor ? (
          <button type="button" className={`${action} text-accent-text`} onClick={() => start(documentAnchor)}>
            <FileText size={13} /> {documentAnchor.region ? "Add note on highlight" : `Add note on page ${documentAnchor.page}`}
          </button>
        ) : null}
        <button type="button" className={action} onClick={() => start({ kind: "lesson" })}>
          <BookmarkPlus size={13} /> Add lesson note
        </button>
      </div>

      <div className="relative min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {error ? <p role="alert" className="mb-3 text-xs text-red-300">{error}</p> : null}
        {notes.isPending ? (
          <div aria-hidden className="space-y-3">
            {[0, 1].map((index) => <div key={index} className="h-16 animate-pulse rounded-xl bg-white/5" />)}
            <p className="sr-only">Loading notes…</p>
          </div>
        ) : notes.data?.length ? (
          <ul aria-label="Notes" className="space-y-2.5">
            {notes.data.map((note) => (
              <li key={note.noteId} className="group rounded-xl border border-line bg-surface p-3 shadow-sm">
                <div className="mb-1.5 flex items-center justify-between gap-2">
                  {note.anchor.kind === "lesson" ? (
                    <span className="rounded-md bg-white/5 px-2 py-0.5 text-[11px] font-medium text-muted">Lesson</span>
                  ) : (
                    <button
                      type="button"
                      aria-label={`Go to ${anchorLabel(note.anchor)}`}
                      onClick={() => {
                        const anchor = note.anchor;
                        if (anchor.kind === "videoTime") onSeek(anchor.resourceId, anchor.seconds);
                        if (anchor.kind === "pdfPage") onOpenPage?.(anchor.resourceId, anchor.page);
                      }}
                      className="inline-flex items-center gap-1 rounded-md bg-accent/10 px-2 py-0.5 text-[11px] font-semibold text-accent-text hover:bg-accent/20"
                    >
                      {note.anchor.kind === "videoTime" ? <Clock size={11} /> : <FileText size={11} />}
                      {anchorLabel(note.anchor)}
                    </button>
                  )}
                  <div className="flex gap-0.5 opacity-60 transition group-hover:opacity-100 group-focus-within:opacity-100">
                    <button type="button" aria-label="Edit note" onClick={() => { setConflict(false); setDraft({ noteId: note.noteId, revision: note.revision, anchor: note.anchor, body: note.body, text: note.text }); }} className="rounded-md p-1 text-muted hover:bg-white/10 hover:text-fg disabled:opacity-40"><Pencil size={13} /></button>
                    <button type="button" aria-label="Delete note" disabled={remove.isPending} onClick={() => remove.mutate(note)} className="rounded-md p-1 text-muted hover:bg-white/10 hover:text-red-300 disabled:opacity-40"><Trash2 size={13} /></button>
                  </div>
                </div>
                <NoteEditor key={`${note.noteId}:${note.revision}`} body={note.body} editable={false} label="Note" />
              </li>
            ))}
          </ul>
        ) : (
          <div className="grid place-items-center px-4 py-10 text-center">
            <NotebookPen size={22} className="mb-2 text-faint" />
            <p className="text-sm text-muted">No notes for this lesson yet.</p>
            <p className="mt-1 max-w-60 text-xs text-subtle">
              {videoResourceId ? "Press N while watching to capture the moment, or add a note for the whole lesson." : "Capture an idea for this lesson, a PDF page, or a highlighted area."}
            </p>
          </div>
        )}
      </div>
      </>
      )}
    </section>
  );
}
