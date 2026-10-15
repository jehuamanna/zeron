import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Highlighter } from "lucide-react";
import { lazy, Suspense, useCallback, useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type Region } from "../../services/library";

const PdfCanvas = lazy(() => import("./PdfCanvas"));

/** A lesson PDF with page navigation and area highlighting. */
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
  const failed = useCallback(() => setRenderFailed(true), []);

  if (document.isPending) return <p className="text-sm text-slate-400">Opening {title}…</p>;
  if (document.isError) {
    return <p role="alert" className="text-sm text-red-300">{asCommandError(document.error)?.message ?? "This document could not be opened."}</p>;
  }
  return (
    <div aria-label={`Document ${title}`} role="region">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3 text-sm">
        <p className="font-medium">{title}</p>
        <div className="flex items-center gap-2">
          <Button variant="ghost" aria-label="Previous page" disabled={page <= 1} onClick={() => onPage(page - 1)}><ChevronLeft size={16} /></Button>
          <span aria-live="polite">Page {page}{pages ? ` of ${pages}` : ""}</span>
          <Button variant="ghost" aria-label="Next page" disabled={pages !== null && page >= pages} onClick={() => onPage(page + 1)}><ChevronRight size={16} /></Button>
          <Button variant="ghost" aria-pressed={drawing} onClick={() => setDrawing(!drawing)}><Highlighter size={15} /> Highlight area</Button>
        </div>
      </div>
      {drawing ? <p className="mb-2 text-xs text-amber-200">Drag over the page to highlight an area, then add a note for it.</p> : null}
      {pendingRegion ? (
        <p className="mb-2 text-xs text-amber-200">
          Area highlighted on page {page}. Use “Add note on highlight” below, or{" "}
          <button type="button" className="underline" onClick={() => onRegion(null)}>clear it</button>.
        </p>
      ) : null}
      {renderFailed ? (
        <p role="alert" className="text-sm text-red-300">This PDF could not be displayed.</p>
      ) : (
        <Suspense fallback={<p className="text-sm text-slate-400">Loading the PDF viewer…</p>}>
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
          />
        </Suspense>
      )}
    </div>
  );
}
