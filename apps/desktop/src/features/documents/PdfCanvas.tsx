import { getDocument, GlobalWorkerOptions, type PDFDocumentProxy, type RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useEffect, useRef, useState, type PointerEvent } from "react";

import type { Region } from "../../services/library";

GlobalWorkerOptions.workerSrc = workerUrl;

export interface PdfCanvasProps {
  data: ArrayBuffer;
  page: number;
  onPageCount: (count: number) => void;
  /** Highlights on this page, in page fractions. */
  highlights: Region[];
  /** While true, dragging draws a highlight rectangle. */
  drawing: boolean;
  onRegion: (region: Region) => void;
  onError: () => void;
}

/** Renders one page with pdf.js. Loaded lazily so pdf.js stays out of the main bundle. */
export default function PdfCanvas({ data, page, onPageCount, highlights, drawing, onRegion, onError }: PdfCanvasProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [current, setCurrent] = useState<Region | null>(null);

  useEffect(() => {
    // pdf.js transfers the buffer to its worker, so give it a copy.
    const task = getDocument({ data: new Uint8Array(data.slice(0)), useWasm: false });
    let cancelled = false;
    task.promise.then(
      (loaded) => {
        if (cancelled) return;
        setPdf(loaded);
        onPageCount(loaded.numPages);
      },
      () => !cancelled && onError(),
    );
    return () => {
      cancelled = true;
      void task.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data]);

  useEffect(() => {
    if (!pdf || !canvas.current) return;
    let task: RenderTask | null = null;
    let cancelled = false;
    void pdf.getPage(page).then((loaded) => {
      if (cancelled || !canvas.current) return;
      const width = canvas.current.parentElement?.clientWidth ?? 800;
      const unscaled = loaded.getViewport({ scale: 1 });
      const viewport = loaded.getViewport({ scale: (width / unscaled.width) * (window.devicePixelRatio || 1) });
      canvas.current.width = viewport.width;
      canvas.current.height = viewport.height;
      canvas.current.style.width = "100%";
      task = loaded.render({ canvas: canvas.current, viewport });
      task.promise.catch(() => undefined);
    }, onError);
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [pdf, page, onError]);

  const point = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return { x: Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)), y: Math.min(1, Math.max(0, (event.clientY - box.top) / box.height)) };
  };
  const rect = (a: { x: number; y: number }, b: { x: number; y: number }): Region => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  });

  return (
    <div className="relative">
      <canvas ref={canvas} aria-label={`Page ${page}`} className="block w-full rounded-xl bg-white" />
      <div
        data-testid="pdf-overlay"
        className={`absolute inset-0 ${drawing ? "cursor-crosshair" : ""}`}
        onPointerDown={(event) => {
          if (!drawing) return;
          const at = point(event);
          setStart(at);
          setCurrent(rect(at, at));
        }}
        onPointerMove={(event) => start && setCurrent(rect(start, point(event)))}
        onPointerUp={(event) => {
          if (!start) return;
          const region = rect(start, point(event));
          setStart(null);
          setCurrent(null);
          if (region.width > 0.01 && region.height > 0.01) onRegion(region);
        }}
      >
        {[...highlights, ...(current ? [current] : [])].map((region, index) => (
          <div
            key={index}
            data-highlight
            className="absolute rounded-sm border border-amber-400/80 bg-amber-300/25"
            style={{ left: `${region.x * 100}%`, top: `${region.y * 100}%`, width: `${region.width * 100}%`, height: `${region.height * 100}%` }}
          />
        ))}
      </div>
    </div>
  );
}
