import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpenText, ChevronDown, ChevronLeft, ChevronRight, ChevronUp, Copy, FilePlus2, Highlighter, Maximize, MessageSquarePlus, Minimize, Minus, PanelLeft, Plus, RotateCw, Search, SquareDashed, Strikethrough, Trash2, Underline, X } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { asCommandError, libraryApi, type HighlightColor, type HighlightStyle, type Note, type NoteAnchor, type Region, type TextHighlight } from "../../services/library";
import { HIGHLIGHT_COLORS, quoteBody } from "./pdfGeometry";
import type { PageMark, PageTheme, SearchHit, TextSelection } from "./PdfDocument";
import type { ListedMark, SidebarTab } from "./PdfSidebar";
import { EMPTY_DOC } from "../notes/NoteEditor";

const PdfDocument = lazy(() => import("./PdfDocument"));

const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];
type PdfAnchor = Extract<NoteAnchor, { kind: "pdfPage" }>;

/** A lesson PDF: continuous pages, selectable text, highlights that are notes, search, and zoom. */
export function DocumentViewer({ courseId, lessonId, resourceId, title, page, onPage, pendingRegion, onRegion, onComment, onEditNote }: {
  courseId: string;
  lessonId: string;
  resourceId: string;
  title: string;
  page: number;
  onPage: (page: number) => void;
  /** A drawn area on the current page waiting for its note. */
  pendingRegion: Region | null;
  onRegion: (page: number, region: Region | null) => void;
  /** Start a note on a highlight in the Notes panel. */
  onComment: (anchor: NoteAnchor, body: string) => void;
  /** Open an existing note in the Notes panel. */
  onEditNote: (noteId: string) => void;
}) {
  const queryClient = useQueryClient();
  const document = useQuery({
    queryKey: ["document", courseId, resourceId],
    queryFn: () => libraryApi.readDocument(courseId, resourceId),
    staleTime: Infinity,
  });
  const notesKey = ["notes", courseId, lessonId];
  const notes = useQuery({ queryKey: notesKey, queryFn: () => libraryApi.notes(courseId, lessonId) });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["notes", courseId] });

  const [pages, setPages] = useState<number | null>(null);
  const [fit, setFit] = useState<"width" | "page">("width");
  const [zoom, setZoom] = useState(1);
  const [theme, setTheme] = useState<PageTheme>("paper");
  const [drawing, setDrawing] = useState(false);
  const [renderFailed, setRenderFailed] = useState(false);
  const [typed, setTyped] = useState(String(page));
  const [selection, setSelection] = useState<TextSelection | null>(null);
  const [open, setOpen] = useState<{ note: Note; at: { x: number; y: number } } | null>(null);
  const [color, setColor] = useState<HighlightColor>("yellow");
  const [searching, setSearching] = useState(false);
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [rotation, setRotation] = useState(0);
  const [spread, setSpread] = useState(false);
  const [sidebar, setSidebar] = useState<SidebarTab | null>(null);
  const [lastTab, setLastTab] = useState<SidebarTab>("pages");
  const [fullscreen, setFullscreen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const failed = useCallback(() => setRenderFailed(true), []);
  useEffect(() => setTyped(String(page)), [page]);
  useEffect(() => {
    const timer = setTimeout(() => setQuery(search), 250);
    return () => clearTimeout(timer);
  }, [search]);

  const pdfNotes = useMemo(
    () => (notes.data ?? []).filter((note): note is Note & { anchor: PdfAnchor } => note.anchor.kind === "pdfPage" && note.anchor.resourceId === resourceId),
    [notes.data, resourceId],
  );
  const marks = pdfNotes.flatMap<PageMark>((note) =>
    note.anchor.highlight
      ? [{ id: note.noteId, page: note.anchor.page, rects: note.anchor.highlight.rects, color: note.anchor.highlight.color, style: note.anchor.highlight.style }]
      : note.anchor.region
        ? [{ id: note.noteId, page: note.anchor.page, rects: [note.anchor.region], color: null, style: "highlight" }]
        : [],
  );

  const create = useMutation({
    mutationFn: ({ anchor, body }: { anchor: PdfAnchor; body: string }) => libraryApi.createNote(courseId, lessonId, anchor, body),
    onSuccess: () => void refresh(),
  });
  const change = useMutation({
    mutationFn: async ({ note, highlight }: { note: Note & { anchor: PdfAnchor }; highlight: TextHighlight }) => {
      const result = await libraryApi.updateNote(note.noteId, note.revision, { ...note.anchor, highlight }, note.body);
      if (result.outcome !== "saved") throw { code: "note_conflict", message: "This highlight changed elsewhere; it was reloaded." };
      return result.note;
    },
    onSuccess: (note) => setOpen((current) => (current ? { ...current, note } : current)),
    onSettled: () => void refresh(),
  });
  const remove = useMutation({
    mutationFn: async (note: Note) => {
      const result = await libraryApi.deleteNote(note.noteId, note.revision);
      if (result.outcome !== "saved") throw { code: "note_conflict", message: "This highlight changed elsewhere; it was reloaded." };
    },
    onSuccess: () => setOpen(null),
    onSettled: () => void refresh(),
  });
  const listed = pdfNotes.flatMap<ListedMark>((note) =>
    note.anchor.highlight
      ? [{ id: note.noteId, page: note.anchor.page, color: note.anchor.highlight.color, text: note.anchor.highlight.quote }]
      : note.anchor.region
        ? [{ id: note.noteId, page: note.anchor.page, color: null, text: note.text || "Area" }]
        : [],
  );
  const error = [create, change, remove].find((mutation) => mutation.isError)?.error;

  const highlightFrom = (current: TextSelection, style: HighlightStyle, chosen: HighlightColor = color): PdfAnchor => ({
    kind: "pdfPage",
    resourceId,
    page: current.page,
    region: null,
    highlight: { color: chosen, style, quote: current.quote, rects: current.rects },
  });
  const clearSelection = () => {
    setSelection(null);
    window.getSelection()?.removeAllRanges();
  };
  const highlight = (style: HighlightStyle, chosen?: HighlightColor) => {
    if (!selection) return;
    if (chosen) setColor(chosen);
    const anchor = highlightFrom(selection, style, chosen);
    create.mutate({ anchor, body: quoteBody(selection.quote) });
    clearSelection();
  };
  const at = (point: { x: number; y: number }) => {
    const box = root.current?.getBoundingClientRect();
    if (!box) return { left: 0, top: 0 };
    return { left: Math.min(Math.max(8, point.x - box.left - 140), box.width - 300), top: Math.min(point.y - box.top + 8, box.height - 120) };
  };

  useEffect(() => {
    const follow = () => setFullscreen(window.document.fullscreenElement === root.current);
    window.document.addEventListener("fullscreenchange", follow);
    return () => window.document.removeEventListener("fullscreenchange", follow);
  }, []);

  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(null);
      setSelection(null);
    };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, []);

  if (document.isPending) return <p className="p-4 text-sm text-muted">Opening {title}…</p>;
  if (document.isError) {
    return <p role="alert" className="p-4 text-sm text-red-300">{asCommandError(document.error)?.message ?? "This document could not be opened."}</p>;
  }

  const goTo = (target: number) => {
    const bounded = Math.max(1, pages ? Math.min(pages, target) : target);
    if (bounded !== page) onPage(bounded);
    else setTyped(String(page));
  };
  const zoomBy = (direction: 1 | -1) => {
    const index = ZOOMS.findIndex((step) => step >= zoom - 0.001);
    setZoom(ZOOMS[Math.min(ZOOMS.length - 1, Math.max(0, (index < 0 ? ZOOMS.length - 1 : index) + direction))]!);
  };
  const hitIndex = hits.findIndex((hit) => hit.page === page);
  const totalHits = hits.reduce((sum, hit) => sum + hit.count, 0);
  const step = (direction: 1 | -1) => {
    if (hits.length === 0) return;
    const next = direction === 1 ? (hits.find((hit) => hit.page > page) ?? hits[0]) : ([...hits].reverse().find((hit) => hit.page < page) ?? hits[hits.length - 1]);
    if (next) onPage(next.page);
  };
  const tool = "grid size-8 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent";
  const openHighlight = open?.note.anchor.kind === "pdfPage" ? open.note.anchor.highlight : undefined;

  return (
    <div ref={root} aria-label={`Document ${title}`} role="region" className="relative flex h-full min-h-0 flex-col bg-bg">
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-line pb-2 text-sm">
        <button
          type="button"
          className={`${tool} ${sidebar ? "bg-accent/10 text-accent-text" : ""}`}
          aria-label={sidebar ? "Hide pages and contents" : "Show pages and contents"}
          aria-pressed={sidebar !== null}
          title="Pages, contents, highlights, and details"
          onClick={() => setSidebar(sidebar ? null : lastTab)}
        >
          <PanelLeft size={16} />
        </button>
        <span aria-hidden className="mx-0.5 h-5 w-px bg-line" />
        <button type="button" className={tool} aria-label="Previous page" disabled={page <= 1} onClick={() => onPage(page - 1)}><ChevronLeft size={16} /></button>
        <form
          className="flex items-center gap-1 text-xs text-muted"
          onSubmit={(event) => {
            event.preventDefault();
            const target = Number.parseInt(typed, 10);
            if (Number.isFinite(target)) goTo(target);
          }}
        >
          <input
            aria-label="Page number"
            inputMode="numeric"
            value={typed}
            onChange={(event) => setTyped(event.target.value.replace(/\D/g, ""))}
            onBlur={() => setTyped(String(page))}
            className="w-12 rounded-md border border-line bg-surface px-1.5 py-1 text-center text-fg"
          />
          <span aria-hidden>/ {pages ?? "…"}</span>
          <span aria-live="polite" className="sr-only">Page {page}{pages ? ` of ${pages}` : ""}</span>
        </form>
        <button type="button" className={tool} aria-label="Next page" disabled={pages !== null && page >= pages} onClick={() => onPage(page + 1)}><ChevronRight size={16} /></button>
        <span aria-hidden className="mx-1 h-5 w-px bg-line" />
        <button type="button" className={tool} aria-label="Zoom out" disabled={zoom <= ZOOMS[0]!} onClick={() => zoomBy(-1)}><Minus size={15} /></button>
        <select
          aria-label="Zoom"
          value={zoom === 1 ? fit : String(zoom)}
          onChange={(event) => {
            const value = event.target.value;
            if (value === "width" || value === "page") {
              setFit(value);
              setZoom(1);
            } else setZoom(Number(value));
          }}
          className="rounded-md border border-line bg-surface px-1.5 py-1 text-xs text-fg"
        >
          <option value="width">Fit width</option>
          <option value="page">Fit page</option>
          {ZOOMS.filter((value) => value !== 1).map((value) => <option key={value} value={String(value)}>{Math.round(value * 100)}%</option>)}
        </select>
        <button type="button" className={tool} aria-label="Zoom in" disabled={zoom >= ZOOMS[ZOOMS.length - 1]!} onClick={() => zoomBy(1)}><Plus size={15} /></button>
        <span aria-hidden className="mx-1 h-5 w-px bg-line" />
        <button type="button" className={tool} aria-label="Rotate pages" title="Rotate clockwise" onClick={() => setRotation((rotation + 90) % 360)}><RotateCw size={15} /></button>
        <button type="button" className={`${tool} ${spread ? "bg-white/10 text-fg" : ""}`} aria-label="Two pages side by side" aria-pressed={spread} onClick={() => setSpread(!spread)}><BookOpenText size={15} /></button>
        <select aria-label="Page colours" value={theme} onChange={(event) => setTheme(event.target.value as PageTheme)} className="rounded-md border border-line bg-surface px-1.5 py-1 text-xs text-fg">
          <option value="paper">Paper</option>
          <option value="sepia">Sepia</option>
          <option value="night">Night</option>
        </select>
        <span aria-hidden className="mx-1 h-5 w-px bg-line" />
        <div role="group" aria-label="Highlight colour" className="flex items-center">
          {HIGHLIGHT_COLORS.map((item) => (
            <button key={item.value} type="button" aria-label={`Use ${item.label.toLowerCase()}`} aria-pressed={color === item.value} title={`${item.label}: the colour for new highlights`} onClick={() => setColor(item.value)} className="grid size-6 place-items-center rounded-full">
              <span className={`size-4 rounded-full ${color === item.value ? "ring-2 ring-fg/50 ring-offset-1 ring-offset-surface" : ""}`} style={{ background: item.css }} />
            </button>
          ))}
        </div>
        <button type="button" className={tool} aria-label={`Add note on page ${page}`} title={`Note on page ${page}`} onClick={() => onComment({ kind: "pdfPage", resourceId, page, region: null }, EMPTY_DOC)}><FilePlus2 size={15} /></button>
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            aria-label="Highlight area"
            aria-pressed={drawing}
            title="Highlight area: draw a box around a figure"
            onClick={() => setDrawing(!drawing)}
            className={`${tool} ${drawing ? "bg-amber-300/20 text-amber-100" : ""}`}
          >
            <SquareDashed size={15} />
          </button>
          <button type="button" className={`${tool} ${searching ? "text-accent-text" : ""}`} aria-label="Find in document" aria-pressed={searching} onClick={() => setSearching(!searching)}><Search size={15} /></button>
          <button
            type="button"
            className={tool}
            aria-label={fullscreen ? "Exit full screen" : "Full screen"}
            onClick={() => (fullscreen ? void window.document.exitFullscreen?.() : void root.current?.requestFullscreen?.().catch(() => undefined))}
          >
            {fullscreen ? <Minimize size={15} /> : <Maximize size={15} />}
          </button>
        </div>
      </div>

      {searching ? (
        <div className="flex shrink-0 items-center gap-2 border-b border-line py-1.5">
          <input
            type="search"
            autoFocus
            aria-label="Search in document"
            placeholder="Search in this PDF"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") step(event.shiftKey ? -1 : 1);
            }}
            className="w-56 rounded-md border border-line bg-surface px-2 py-1 text-sm text-fg"
          />
          <span aria-live="polite" className="text-xs text-muted">
            {query.trim() ? (hits.length ? `${totalHits} ${totalHits === 1 ? "match" : "matches"} on ${hits.length} ${hits.length === 1 ? "page" : "pages"}${hitIndex >= 0 ? ` · page ${hitIndex + 1} of ${hits.length}` : ""}` : "No matches") : ""}
          </span>
          <button type="button" className={tool} aria-label="Previous match" disabled={hits.length === 0} onClick={() => step(-1)}><ChevronUp size={15} /></button>
          <button type="button" className={tool} aria-label="Next match" disabled={hits.length === 0} onClick={() => step(1)}><ChevronDown size={15} /></button>
        </div>
      ) : null}
      {drawing ? <p className="shrink-0 pt-2 text-xs text-amber-200">Drag over a page to box an area, then add a note for it.</p> : null}
      {pendingRegion ? (
        <p className="shrink-0 pt-2 text-xs text-amber-200">
          Area boxed on page {page}. Use “Add note on highlight” in Notes, or{" "}
          <button type="button" className="underline" onClick={() => onRegion(page, null)}>clear it</button>.
        </p>
      ) : null}
      {error ? <p role="alert" className="shrink-0 pt-2 text-xs text-red-300">{asCommandError(error)?.message ?? "The highlight could not be saved."}</p> : null}

      <div className="relative min-h-0 flex-1">
        {renderFailed ? (
          <p role="alert" className="p-4 text-sm text-red-300">This PDF could not be displayed.</p>
        ) : (
          <Suspense fallback={<p className="p-4 text-sm text-muted">Loading the PDF viewer…</p>}>
            <PdfDocument
              data={document.data}
              page={page}
              onPage={onPage}
              onPageCount={setPages}
              fit={fit}
              zoom={zoom}
              theme={theme}
              marks={marks}
              pending={pendingRegion ? [{ page, region: pendingRegion }] : []}
              drawing={drawing}
              onRegion={(target, region) => {
                setDrawing(false);
                onRegion(target, region);
              }}
              onSelect={(next) => {
                setSelection(next);
                if (next) setOpen(null);
              }}
              onMark={(id, point) => {
                const note = pdfNotes.find((candidate) => candidate.noteId === id);
                if (note) setOpen({ note, at: point });
              }}
              search={searching ? query : ""}
              onSearchHits={setHits}
              onError={failed}
              rotation={rotation}
              spread={spread}
              sidebar={sidebar}
              onSidebar={(tab) => {
                setSidebar(tab);
                setLastTab(tab);
              }}
              listed={listed}
            />
          </Suspense>
        )}
      </div>

      {selection ? (
        <div role="toolbar" aria-label="Selected text" className="absolute z-30 flex items-center gap-0.5 rounded-xl border border-line bg-surface p-1 shadow-panel" style={at(selection.at)} onMouseUp={(event) => event.stopPropagation()}>
          {HIGHLIGHT_COLORS.map((item) => (
            <button key={item.value} type="button" aria-label={`Highlight ${item.label.toLowerCase()}`} title={`Highlight ${item.label.toLowerCase()}`} onClick={() => highlight("highlight", item.value)} className="grid size-7 place-items-center rounded-md hover:bg-white/10">
              <span className={`size-4 rounded-full ${item.value === color ? "ring-2 ring-fg/40 ring-offset-1 ring-offset-surface" : ""}`} style={{ background: item.css }} />
            </button>
          ))}
          <span aria-hidden className="mx-0.5 h-5 w-px bg-line" />
          <button type="button" className={tool} aria-label="Underline" title="Underline" onClick={() => highlight("underline")}><Underline size={15} /></button>
          <button type="button" className={tool} aria-label="Strike through" title="Strike through" onClick={() => highlight("strike")}><Strikethrough size={15} /></button>
          <button
            type="button"
            className={tool}
            aria-label="Highlight and comment"
            title="Highlight and comment"
            onClick={() => {
              onComment(highlightFrom(selection, "highlight"), quoteBody(selection.quote));
              clearSelection();
            }}
          >
            <MessageSquarePlus size={15} />
          </button>
          <button type="button" className={tool} aria-label="Copy" title="Copy" onClick={() => void navigator.clipboard?.writeText(selection.quote).then(clearSelection)}><Copy size={15} /></button>
          <button type="button" className={tool} aria-label="Close" onClick={clearSelection}><X size={15} /></button>
        </div>
      ) : null}

      {open && openHighlight ? (
        <div role="dialog" aria-label="Highlight" className="absolute z-30 w-72 rounded-xl border border-line bg-surface p-2 shadow-panel" style={at(open.at)} onMouseUp={(event) => event.stopPropagation()}>
          <div className="flex items-center gap-0.5">
            {HIGHLIGHT_COLORS.map((item) => (
              <button
                key={item.value}
                type="button"
                aria-label={`Colour ${item.label.toLowerCase()}`}
                aria-pressed={openHighlight.color === item.value}
                disabled={change.isPending}
                onClick={() => change.mutate({ note: open.note as Note & { anchor: PdfAnchor }, highlight: { ...openHighlight, color: item.value } })}
                className="grid size-7 place-items-center rounded-md hover:bg-white/10"
              >
                <span className={`size-4 rounded-full ${openHighlight.color === item.value ? "ring-2 ring-fg/40 ring-offset-1 ring-offset-surface" : ""}`} style={{ background: item.css }} />
              </button>
            ))}
            <span aria-hidden className="mx-0.5 h-5 w-px bg-line" />
            {([["highlight", Highlighter, "Highlight style"], ["underline", Underline, "Underline style"], ["strike", Strikethrough, "Strike-through style"]] as const).map(([value, Icon, label]) => (
              <button
                key={value}
                type="button"
                aria-label={label}
                aria-pressed={openHighlight.style === value}
                disabled={change.isPending}
                onClick={() => change.mutate({ note: open.note as Note & { anchor: PdfAnchor }, highlight: { ...openHighlight, style: value } })}
                className={`${tool} ${openHighlight.style === value ? "bg-white/10 text-fg" : ""}`}
              >
                <Icon size={14} />
              </button>
            ))}
            <button type="button" className={`${tool} ml-auto`} aria-label="Close" onClick={() => setOpen(null)}><X size={15} /></button>
          </div>
          <p className="mt-2 line-clamp-3 border-l-2 pl-2 font-reading text-[13px] italic text-fg-soft" style={{ borderColor: HIGHLIGHT_COLORS.find((item) => item.value === openHighlight.color)?.css }}>
            {openHighlight.quote}
          </p>
          <div className="mt-2 flex items-center gap-1">
            <button type="button" className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-accent-text hover:bg-accent/10" onClick={() => { onEditNote(open.note.noteId); setOpen(null); }}>
              <MessageSquarePlus size={13} /> Comment
            </button>
            <button type="button" className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted hover:bg-white/10 hover:text-fg" onClick={() => void navigator.clipboard?.writeText(openHighlight.quote)}>
              <Copy size={13} /> Copy
            </button>
            <button type="button" disabled={remove.isPending} className="ml-auto inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted hover:bg-white/10 hover:text-red-300" onClick={() => remove.mutate(open.note)}>
              <Trash2 size={13} /> Delete
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
