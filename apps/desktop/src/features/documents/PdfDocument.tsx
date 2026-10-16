import { getDocument, GlobalWorkerOptions, TextLayer, type PDFDocumentProxy, type PDFPageProxy, type RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import type { HighlightColor, HighlightStyle, Region } from "../../services/library";
import { cleanQuote, colorCss, hits, rotateRegion, selectionRegions, unrotateRegion } from "./pdfGeometry";
import { PdfSidebar, type ListedMark, type SidebarTab } from "./PdfSidebar";

GlobalWorkerOptions.workerSrc = workerUrl;

/** Space between pages and around them, in CSS pixels. */
const GAP = 16;

export type PageTheme = "paper" | "sepia" | "night";
const THEME_FILTER: Record<PageTheme, string> = {
  paper: "none",
  sepia: "sepia(0.35) brightness(0.97)",
  night: "invert(0.9) hue-rotate(180deg)",
};

/** A saved highlight or area drawn over a page. */
export interface PageMark {
  id: string;
  page: number;
  rects: Region[];
  /** Area notes have no colour; they are drawn as an amber box. */
  color: HighlightColor | null;
  style: HighlightStyle;
}

/** Text the learner selected on one page. */
export interface TextSelection {
  page: number;
  quote: string;
  rects: Region[];
  /** Where to show the toolbar, in client pixels. */
  at: { x: number; y: number };
}

export interface SearchHit {
  page: number;
  count: number;
  snippet: string;
}

export interface PdfDocumentProps {
  data: ArrayBuffer;
  /** The page to show; changing it scrolls there. */
  page: number;
  /** The page most in view, as the reader scrolls. */
  onPage: (page: number) => void;
  onPageCount: (count: number) => void;
  fit: "width" | "page";
  zoom: number;
  theme: PageTheme;
  marks: PageMark[];
  /** Areas being drawn or waiting for a note. */
  pending: Array<{ page: number; region: Region }>;
  drawing: boolean;
  onRegion: (page: number, region: Region) => void;
  onSelect: (selection: TextSelection | null) => void;
  onMark: (id: string, at: { x: number; y: number }) => void;
  search: string;
  onSearchHits: (hits: SearchHit[]) => void;
  onError: () => void;
  /** Clockwise turn of every page: 0, 90, 180, or 270. */
  rotation: number;
  /** Two pages side by side. */
  spread: boolean;
  /** The open sidebar tab, or null when the sidebar is hidden. */
  sidebar: SidebarTab | null;
  onSidebar: (tab: SidebarTab) => void;
  /** Highlights and areas for the sidebar list. */
  listed: ListedMark[];
}

/**
 * Every page of a PDF in one scrolling column. Pages near the view render a
 * canvas and a selectable text layer; the rest are empty boxes of the right
 * size. Loaded lazily so pdf.js stays out of the main bundle.
 */
export default function PdfDocument(props: PdfDocumentProps) {
  const { data, page, onPage, onPageCount, fit, zoom, theme, marks, pending, drawing, onRegion, onSelect, onMark, search, onSearchHits, onError, rotation, spread, sidebar, onSidebar, listed } = props;
  const scroller = useRef<HTMLDivElement>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [sizes, setSizes] = useState<Array<{ width: number; height: number }>>([]);
  const [box, setBox] = useState({ width: 800, height: 1000 });
  const [near, setNear] = useState<Set<number>>(new Set([1, 2]));
  // The view starts at the top; a different requested page scrolls there once pages are laid out.
  const reported = useRef(1);
  const texts = useRef(new Map<number, string>());

  useEffect(() => {
    // pdf.js transfers the buffer to its worker, so give it a copy.
    const task = getDocument({ data: new Uint8Array(data.slice(0)), useWasm: false });
    let cancelled = false;
    task.promise.then(
      async (loaded) => {
        if (cancelled) return;
        const first = (await loaded.getPage(1)).getViewport({ scale: 1 });
        if (cancelled) return;
        setSizes(Array.from({ length: loaded.numPages }, () => ({ width: first.width, height: first.height })));
        setPdf(loaded);
        onPageCount(loaded.numPages);
        // Learn every page's real size in the background; most PDFs are uniform.
        for (let number = 2; number <= loaded.numPages && !cancelled; number += 1) {
          const viewport = (await loaded.getPage(number)).getViewport({ scale: 1 });
          if (viewport.width !== first.width || viewport.height !== first.height) {
            setSizes((current) => current.map((size, index) => (index === number - 1 ? { width: viewport.width, height: viewport.height } : size)));
          }
        }
      },
      () => !cancelled && onError(),
    );
    return () => {
      cancelled = true;
      void task.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data]);

  // The reading area's size decides the fitted scale.
  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const measure = () => setBox({ width: element.clientWidth, height: element.clientHeight });
    measure();
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const turned = rotation % 180 !== 0;
  const shown = (size: { width: number; height: number }) => (turned ? { width: size.height, height: size.width } : size);
  const base = shown(sizes[0] ?? { width: 612, height: 792 });
  const across = spread ? 2 : 1;
  const fitted = fit === "page"
    ? Math.min((box.width - (across + 1) * GAP) / (across * base.width), (box.height - 2 * GAP) / base.height)
    : (box.width - (across + 1) * GAP) / (across * base.width);
  const scale = Math.max(0.1, fitted * zoom);

  // Which pages are near the view, and which one is current.
  const pageElements = useRef(new Map<number, HTMLDivElement>());
  const measureView = useCallback(() => {
    const element = scroller.current;
    if (!element || sizes.length === 0) return;
    const top = element.scrollTop;
    const bottom = top + element.clientHeight;
    const margin = element.clientHeight;
    const nextNear = new Set<number>();
    let current = 1;
    for (const [number, pageElement] of pageElements.current) {
      const start = pageElement.offsetTop;
      const end = start + pageElement.offsetHeight;
      if (end >= top - margin && start <= bottom + margin) nextNear.add(number);
      if (start <= top + element.clientHeight * 0.35) current = Math.max(current, number);
    }
    setNear((previous) => (previous.size === nextNear.size && [...nextNear].every((number) => previous.has(number)) ? previous : nextNear));
    if (current !== reported.current) {
      reported.current = current;
      onPage(current);
    }
  }, [sizes.length, onPage]);

  useEffect(() => {
    measureView();
  }, [measureView, scale, spread, rotation]);

  // Scroll when the page is changed from outside (toolbar, notes, search).
  useEffect(() => {
    if (page === reported.current) return;
    const target = pageElements.current.get(page);
    if (target && scroller.current) {
      reported.current = page;
      scroller.current.scrollTop = target.offsetTop - GAP / 2;
    }
  }, [page, sizes.length]);

  // Search page text, page by page, in the background.
  useEffect(() => {
    const query = search.trim().toLowerCase();
    if (!pdf || !query) {
      onSearchHits([]);
      return;
    }
    let cancelled = false;
    void (async () => {
      const found: SearchHit[] = [];
      for (let number = 1; number <= pdf.numPages && !cancelled; number += 1) {
        let text = texts.current.get(number);
        if (text === undefined) {
          const content = await (await pdf.getPage(number)).getTextContent();
          text = content.items.map((item) => ("str" in item ? item.str : "")).join(" ").replace(/\s+/g, " ");
          texts.current.set(number, text);
        }
        const lower = text.toLowerCase();
        let count = 0;
        let at = lower.indexOf(query);
        const first = at;
        while (at >= 0) {
          count += 1;
          at = lower.indexOf(query, at + query.length);
        }
        if (count > 0) found.push({ page: number, count, snippet: text.slice(Math.max(0, first - 40), first + query.length + 60).trim() });
      }
      if (!cancelled) onSearchHits(found);
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pdf, search]);

  const finishSelection = (event: React.MouseEvent) => {
    if (drawing) return;
    const selection = window.getSelection();
    const pageElement = (event.target as Element).closest<HTMLElement>("[data-page]");
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
      onSelect(null);
      // A click on a saved highlight opens it.
      if (pageElement) {
        const number = Number(pageElement.dataset.page);
        const rect = pageElement.getBoundingClientRect();
        const x = (event.clientX - rect.left) / rect.width;
        const y = (event.clientY - rect.top) / rect.height;
        const mark = [...marks].reverse().find((candidate) => candidate.page === number && candidate.color && hits(candidate.rects.map((region) => rotateRegion(region, rotation)), x, y));
        if (mark) onMark(mark.id, { x: event.clientX, y: event.clientY });
      }
      return;
    }
    const range = selection.getRangeAt(0);
    const startPage = (range.startContainer.parentElement ?? null)?.closest<HTMLElement>("[data-page]") ?? pageElement;
    const quote = cleanQuote(selection.toString());
    if (!startPage || !quote) {
      onSelect(null);
      return;
    }
    const pageBox = startPage.getBoundingClientRect();
    const regions = selectionRegions(Array.from(range.getClientRects()), pageBox).map((region) => unrotateRegion(region, rotation));
    if (regions.length === 0) {
      onSelect(null);
      return;
    }
    const last = range.getBoundingClientRect();
    onSelect({ page: Number(startPage.dataset.page), quote, rects: regions, at: { x: Math.min(last.right, pageBox.right), y: last.bottom } });
  };

  const pageNumbers = sizes.map((_, index) => index + 1);
  const rows = spread ? pageNumbers.reduce<number[][]>((all, number) => (number % 2 === 1 ? [...all, [number]] : [...all.slice(0, -1), [...all[all.length - 1]!, number]]), []) : pageNumbers.map((number) => [number]);
  const renderPage = (number: number) => {
    const size = shown(sizes[number - 1]!);
    return (
      <PdfPage
        key={number}
        ref={(element) => {
          if (element) pageElements.current.set(number, element);
          else pageElements.current.delete(number);
        }}
        pdf={pdf}
        number={number}
        width={size.width * scale}
        height={size.height * scale}
        scale={scale}
        rotation={rotation}
        rendered={near.has(number)}
        theme={theme}
        marks={marks.filter((mark) => mark.page === number)}
        pending={pending.filter((item) => item.page === number).map((item) => item.region)}
        drawing={drawing}
        onRegion={(region) => onRegion(number, unrotateRegion(region, rotation))}
        search={search.trim().toLowerCase()}
        onError={onError}
      />
    );
  };

  return (
    <div className="flex h-full min-h-0">
      {sidebar && pdf ? (
        <div className="w-60 shrink-0">
          <PdfSidebar pdf={pdf} tab={sidebar} onTab={onSidebar} page={page} onJump={onPage} marks={listed} bytes={data.byteLength} rotation={rotation} />
        </div>
      ) : null}
      <div
        ref={scroller}
        data-testid="pdf-scroller"
        className="relative h-full min-w-0 flex-1 overflow-auto bg-sunken"
        onScroll={() => {
          onSelect(null);
          window.requestAnimationFrame(measureView);
        }}
        onMouseUp={finishSelection}
      >
        <div className="flex min-w-fit flex-col items-center" style={{ gap: GAP, padding: GAP }}>
          {rows.map((row) => (
            <div key={row[0]} className="flex items-start" style={{ gap: GAP }}>
              {row.map(renderPage)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function PdfPage({ ref, pdf, number, width, height, scale, rotation, rendered, theme, marks, pending, drawing, onRegion, search, onError }: {
  ref: (element: HTMLDivElement | null) => void;
  pdf: PDFDocumentProxy | null;
  number: number;
  width: number;
  height: number;
  scale: number;
  rotation: number;
  rendered: boolean;
  theme: PageTheme;
  marks: PageMark[];
  pending: Region[];
  drawing: boolean;
  onRegion: (region: Region) => void;
  search: string;
  onError: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const text = useRef<HTMLDivElement>(null);
  const [textReady, setTextReady] = useState(0);

  useEffect(() => {
    if (!pdf || !rendered || !canvas.current || !text.current) return;
    let cancelled = false;
    let task: RenderTask | null = null;
    let layer: TextLayer | null = null;
    const textContainer = text.current;
    void pdf.getPage(number).then(async (loaded: PDFPageProxy) => {
      if (cancelled || !canvas.current) return;
      const turn = (loaded.rotate + rotation) % 360;
      const viewport = loaded.getViewport({ scale, rotation: turn });
      const ratio = window.devicePixelRatio || 1;
      const output = loaded.getViewport({ scale: scale * ratio, rotation: turn });
      canvas.current.width = Math.floor(output.width);
      canvas.current.height = Math.floor(output.height);
      task = loaded.render({ canvas: canvas.current, viewport: output });
      task.promise.catch(() => undefined);

      textContainer.replaceChildren();
      textContainer.style.setProperty("--total-scale-factor", String(scale));
      textContainer.style.setProperty("--scale-round-x", "1px");
      textContainer.style.setProperty("--scale-round-y", "1px");
      layer = new TextLayer({ textContentSource: loaded.streamTextContent(), container: textContainer, viewport });
      await layer.render().catch(() => undefined);
      if (!cancelled) setTextReady((count) => count + 1);
    }, onError);
    return () => {
      cancelled = true;
      task?.cancel();
      layer?.cancel();
    };
  }, [pdf, number, scale, rotation, rendered, onError]);

  // Mark text-layer pieces that contain the search words.
  useEffect(() => {
    const container = text.current;
    if (!container) return;
    for (const span of container.querySelectorAll("span")) {
      span.classList.toggle("zr-hit", search.length > 0 && (span.textContent ?? "").toLowerCase().includes(search));
    }
  }, [search, textReady]);

  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [current, setCurrent] = useState<Region | null>(null);
  const point = (event: ReactPointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width)), y: Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height)) };
  };
  const between = (a: { x: number; y: number }, b: { x: number; y: number }): Region => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  });

  return (
    <div ref={ref} data-page={number} className="relative shrink-0 bg-[#fff] shadow-panel" style={{ width, height }}>
      {rendered ? (
        <>
          <canvas ref={canvas} aria-label={`Page ${number}`} className="absolute inset-0 h-full w-full" style={{ filter: THEME_FILTER[theme] }} />
          {/* Highlights sit under the text so the text stays selectable. */}
          <div aria-hidden className="pointer-events-none absolute inset-0">
            {marks.flatMap((mark) =>
              mark.rects.map((stored) => rotateRegion(stored, rotation)).map((rect, index) => (
                <div
                  key={`${mark.id}-${index}`}
                  data-highlight={mark.id}
                  className="absolute"
                  style={{
                    left: `${rect.x * 100}%`,
                    top: `${rect.y * 100}%`,
                    width: `${rect.width * 100}%`,
                    height: `${rect.height * 100}%`,
                    ...(mark.color === null
                      ? { border: "1px solid rgb(245 158 11 / 0.8)", background: "rgb(252 211 77 / 0.2)", borderRadius: 2 }
                      : mark.style === "highlight"
                        ? { background: colorCss(mark.color), opacity: 0.38, mixBlendMode: theme === "night" ? "screen" : "multiply", borderRadius: 2 }
                        : mark.style === "underline"
                          ? { borderBottom: `2px solid ${colorCss(mark.color)}` }
                          : { background: `linear-gradient(transparent 46%, ${colorCss(mark.color)} 46%, ${colorCss(mark.color)} 58%, transparent 58%)` }),
                  }}
                />
              )),
            )}
          </div>
          <div ref={text} className="textLayer" />
        </>
      ) : (
        <div ref={text} className="hidden" />
      )}
      {pending.map((stored) => rotateRegion(stored, rotation)).map((region, index) => (
        <div
          key={`pending-${index}`}
          aria-hidden
          className="pointer-events-none absolute rounded-sm border border-amber-400/80 bg-amber-300/25"
          style={{ left: `${region.x * 100}%`, top: `${region.y * 100}%`, width: `${region.width * 100}%`, height: `${region.height * 100}%` }}
        />
      ))}
      {drawing ? (
        <div
          data-testid="pdf-overlay"
          className="absolute inset-0 z-10 cursor-crosshair"
          onPointerDown={(event) => {
            const at = point(event);
            setStart(at);
            setCurrent(between(at, at));
          }}
          onPointerMove={(event) => start && setCurrent(between(start, point(event)))}
          onPointerUp={(event) => {
            if (!start) return;
            const region = between(start, point(event));
            setStart(null);
            setCurrent(null);
            if (region.width > 0.01 && region.height > 0.01) onRegion(region);
          }}
        >
          {current ? (
            <div className="absolute rounded-sm border border-amber-400/80 bg-amber-300/25" style={{ left: `${current.x * 100}%`, top: `${current.y * 100}%`, width: `${current.width * 100}%`, height: `${current.height * 100}%` }} />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
