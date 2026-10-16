import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Highlighter, Minus, Plus, StretchHorizontal, StretchVertical } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";

import { asCommandError, libraryApi, type Region } from "../../services/library";

const PdfCanvas = lazy(() => import("./PdfCanvas"));

const ZOOM_STEPS = [0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3];

/** A lesson PDF with page navigation, zoom, and area highlighting. */
export function DocumentViewer({ courseId, resourceId, title, page, onPage, highlights, pendingRegion, onRegion }: {
  courseId: string;
  resourceId: string;
  title: string;
  page: number;
  onPage: (page: number) => void;
  highlights: Region[];
  /** A drawn area waiting for its note. */
  pendingRegion: Region | null;
  onRegion: (region: Region | null) => void;
}) {
  const document = useQuery({
    queryKey: ["document", courseId, resourceId],
    queryFn: () => libraryApi.readDocument(courseId, resourceId),
    staleTime: Infinity,
  });
  const [pages, setPages] = useState<number | null>(null);
  const [drawing, setDrawing] = useState(false);
  const [renderFailed, setRenderFailed] = useState(false);
  const [fit, setFit] = useState<"width" | "page">("width");
  const [zoom, setZoom] = useState(1);
  const [typed, setTyped] = useState(String(page));
  const failed = useCallback(() => setRenderFailed(true), []);
  useEffect(() => setTyped(String(page)), [page]);

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
    const index = ZOOM_STEPS.findIndex((step) => step >= zoom - 0.001);
    const next = ZOOM_STEPS[Math.min(ZOOM_STEPS.length - 1, Math.max(0, (index < 0 ? ZOOM_STEPS.length - 1 : index) + direction))]!;
    setZoom(next);
  };
  const tool = "grid size-8 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent";

  return (
    <div aria-label={`Document ${title}`} role="region" className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-line pb-2 text-sm">
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
            className="w-11 rounded-md border border-line bg-surface px-1.5 py-1 text-center text-fg"
          />
          <span aria-hidden>/ {pages ?? "…"}</span>
          <span aria-live="polite" className="sr-only">Page {page}{pages ? ` of ${pages}` : ""}</span>
        </form>
        <button type="button" className={tool} aria-label="Next page" disabled={pages !== null && page >= pages} onClick={() => onPage(page + 1)}><ChevronRight size={16} /></button>
        <span aria-hidden className="mx-1 h-5 w-px bg-line" />
        <button type="button" className={tool} aria-label="Zoom out" disabled={zoom <= ZOOM_STEPS[0]!} onClick={() => zoomBy(-1)}><Minus size={15} /></button>
        <button type="button" className="rounded-md px-1.5 py-1 text-xs text-muted hover:bg-white/10 hover:text-fg" title="Reset zoom" onClick={() => setZoom(1)}>
          {Math.round(zoom * 100)}%
        </button>
        <button type="button" className={tool} aria-label="Zoom in" disabled={zoom >= ZOOM_STEPS[ZOOM_STEPS.length - 1]!} onClick={() => zoomBy(1)}><Plus size={15} /></button>
        <button
          type="button"
          className={tool}
          aria-label={fit === "width" ? "Fit whole page" : "Fit page width"}
          title={fit === "width" ? "Fit whole page" : "Fit page width"}
          onClick={() => {
            setFit(fit === "width" ? "page" : "width");
            setZoom(1);
          }}
        >
          {fit === "width" ? <StretchVertical size={15} /> : <StretchHorizontal size={15} />}
        </button>
        <button
          type="button"
          aria-pressed={drawing}
          onClick={() => setDrawing(!drawing)}
          className={`ml-auto inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium ${drawing ? "bg-amber-300/20 text-amber-100" : "text-muted hover:bg-white/10 hover:text-fg"}`}
        >
          <Highlighter size={14} /> Highlight area
        </button>
      </div>
      {drawing ? <p className="shrink-0 pt-2 text-xs text-amber-200">Drag over the page to highlight an area, then add a note for it.</p> : null}
      {pendingRegion ? (
        <p className="shrink-0 pt-2 text-xs text-amber-200">
          Area highlighted on page {page}. Use “Add note on highlight” in Notes, or{" "}
          <button type="button" className="underline" onClick={() => onRegion(null)}>clear it</button>.
        </p>
      ) : null}
      <div className="relative min-h-0 flex-1 overflow-auto bg-sunken p-4">
        {renderFailed ? (
          <p role="alert" className="text-sm text-red-300">This PDF could not be displayed.</p>
        ) : (
          <Suspense fallback={<p className="text-sm text-muted">Loading the PDF viewer…</p>}>
            <PdfCanvas
              data={document.data}
              page={page}
              onPageCount={setPages}
              highlights={pendingRegion ? [...highlights, pendingRegion] : highlights}
              drawing={drawing}
              onRegion={(region) => {
                setDrawing(false);
                onRegion(region);
              }}
              onError={failed}
              fit={fit}
              zoom={zoom}
            />
          </Suspense>
        )}
      </div>
    </div>
  );
}
