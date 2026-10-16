import { getDocument, GlobalWorkerOptions, type PDFDocumentProxy, type RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useEffect, useRef, useState, type PointerEvent } from "react";

import type { Region } from "../../services/library";

GlobalWorkerOptions.workerSrc = workerUrl;

/** Space kept around the page inside the reading area, in CSS pixels. */
const PAGE_GUTTER = 32;

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
  /** Fit the page to the viewport's width or whole page, then scale by `zoom`. */
  fit?: "width" | "page";
  zoom?: number;
}

/** Renders one page with pdf.js. Loaded lazily so pdf.js stays out of the main bundle. */
export default function PdfCanvas({ data, page, onPageCount, highlights, drawing, onRegion, onError, fit = "width", zoom = 1 }: PdfCanvasProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<{ width: number; height: number } | null>(null);
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

  // Re-render when the reading area changes size, e.g. a panel opens.
  useEffect(() => {
    const viewport = frame.current?.parentElement;
    if (!viewport) return;
    const measure = () => setBox({ width: viewport.clientWidth, height: viewport.clientHeight });
    measure();
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(measure);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!pdf || !canvas.current) return;
    let task: RenderTask | null = null;
    let cancelled = false;
    void pdf.getPage(page).then((loaded) => {
      if (cancelled || !canvas.current) return;
      const unscaled = loaded.getViewport({ scale: 1 });
      const width = Math.max(200, (box?.width ?? 800) - PAGE_GUTTER);
      const height = Math.max(200, (box?.height ?? 1000) - PAGE_GUTTER);
      const fitted = fit === "page" ? Math.min(width / unscaled.width, height / unscaled.height) : width / unscaled.width;
      const cssScale = fitted * zoom;
      const viewport = loaded.getViewport({ scale: cssScale * (window.devicePixelRatio || 1) });
      canvas.current.width = viewport.width;
      canvas.current.height = viewport.height;
      canvas.current.style.width = `${Math.floor(unscaled.width * cssScale)}px`;
      if (frame.current) frame.current.style.width = canvas.current.style.width;
      task = loaded.render({ canvas: canvas.current, viewport });
      task.promise.catch(() => undefined);
    }, onError);
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [pdf, page, onError, box, fit, zoom]);

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
    <div ref={frame} className="relative mx-auto max-w-none shadow-panel">
      <canvas ref={canvas} aria-label={`Page ${page}`} className="block bg-[#fff]" />
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
