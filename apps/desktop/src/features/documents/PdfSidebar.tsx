import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist";
import { useEffect, useRef, useState } from "react";

import type { HighlightColor } from "../../services/library";
import { colorCss } from "./pdfGeometry";

export type SidebarTab = "pages" | "contents" | "highlights" | "details";

/** A highlight or area note listed in the sidebar. */
export interface ListedMark {
  id: string;
  page: number;
  color: HighlightColor | null;
  text: string;
}

const TABS: Array<[SidebarTab, string]> = [
  ["pages", "Pages"],
  ["contents", "Contents"],
  ["highlights", "Highlights"],
  ["details", "Details"],
];

/** Thumbnails, the PDF's own outline, the learner's highlights, and file details. */
export function PdfSidebar({ pdf, tab, onTab, page, onJump, marks, bytes, rotation }: {
  pdf: PDFDocumentProxy;
  tab: SidebarTab;
  onTab: (tab: SidebarTab) => void;
  page: number;
  onJump: (page: number) => void;
  marks: ListedMark[];
  bytes: number;
  rotation: number;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col border-r border-line bg-surface">
      <div role="tablist" aria-label="Document sidebar" className="flex shrink-0 border-b border-line px-1">
        {TABS.map(([value, label]) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-selected={tab === value}
            onClick={() => onTab(value)}
            className={`relative flex-1 px-1.5 py-2 text-xs font-medium ${tab === value ? "text-fg" : "text-muted hover:text-fg"}`}
          >
            {label}
            {tab === value ? <span aria-hidden className="absolute inset-x-1.5 bottom-0 h-0.5 rounded-full bg-accent" /> : null}
          </button>
        ))}
      </div>
      <div role="tabpanel" className="relative min-h-0 flex-1 overflow-y-auto">
        {tab === "pages" ? <Thumbnails pdf={pdf} page={page} onJump={onJump} rotation={rotation} /> : null}
        {tab === "contents" ? <Outline pdf={pdf} onJump={onJump} /> : null}
        {tab === "highlights" ? <Highlights marks={marks} page={page} onJump={onJump} /> : null}
        {tab === "details" ? <Details pdf={pdf} bytes={bytes} /> : null}
      </div>
    </div>
  );
}

function Thumbnails({ pdf, page, onJump, rotation }: { pdf: PDFDocumentProxy; page: number; onJump: (page: number) => void; rotation: number }) {
  const list = useRef<HTMLOListElement>(null);
  const [near, setNear] = useState<Set<number>>(new Set([1, 2, 3, 4]));

  // Render only thumbnails near the visible part of the list.
  useEffect(() => {
    const root = list.current?.parentElement;
    if (!root || typeof IntersectionObserver !== "function") return;
    const observer = new IntersectionObserver(
      (entries) =>
        setNear((previous) => {
          const next = new Set(previous);
          for (const entry of entries) {
            const number = Number((entry.target as HTMLElement).dataset.thumb);
            if (entry.isIntersecting) next.add(number);
            else next.delete(number);
          }
          return next;
        }),
      { root, rootMargin: "400px 0px" },
    );
    for (const item of list.current!.querySelectorAll("[data-thumb]")) observer.observe(item);
    return () => observer.disconnect();
  }, [pdf]);

  // Keep the current page's thumbnail in view.
  useEffect(() => {
    list.current?.querySelector(`[data-thumb="${page}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [page]);

  return (
    <ol ref={list} aria-label="Pages" className="space-y-3 p-3">
      {Array.from({ length: pdf.numPages }, (_, index) => index + 1).map((number) => (
        <li key={number} data-thumb={number} className="text-center">
          <button
            type="button"
            aria-label={`Page ${number}`}
            aria-current={number === page ? "page" : undefined}
            onClick={() => onJump(number)}
            className={`mx-auto block overflow-hidden rounded-md border-2 bg-[#fff] ${number === page ? "border-accent" : "border-transparent hover:border-line"}`}
          >
            {near.has(number) ? <Thumbnail pdf={pdf} number={number} rotation={rotation} /> : <div className="h-40 w-28" />}
          </button>
          <span className={`mt-1 block text-[11px] ${number === page ? "font-semibold text-accent-text" : "text-subtle"}`}>{number}</span>
        </li>
      ))}
    </ol>
  );
}

function Thumbnail({ pdf, number, rotation }: { pdf: PDFDocumentProxy; number: number; rotation: number }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let task: RenderTask | null = null;
    let cancelled = false;
    void pdf.getPage(number).then((loaded) => {
      if (cancelled || !canvas.current) return;
      const unscaled = loaded.getViewport({ scale: 1, rotation: (loaded.rotate + rotation) % 360 });
      const viewport = loaded.getViewport({ scale: (160 / unscaled.width) * (window.devicePixelRatio || 1), rotation: (loaded.rotate + rotation) % 360 });
      canvas.current.width = Math.floor(viewport.width);
      canvas.current.height = Math.floor(viewport.height);
      canvas.current.style.width = "160px";
      task = loaded.render({ canvas: canvas.current, viewport });
      task.promise.catch(() => undefined);
    });
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [pdf, number, rotation]);
  return <canvas ref={canvas} aria-hidden className="block" />;
}

interface OutlineItem {
  title: string;
  dest: string | unknown[] | null;
  items: OutlineItem[];
}

function Outline({ pdf, onJump }: { pdf: PDFDocumentProxy; onJump: (page: number) => void }) {
  const [outline, setOutline] = useState<OutlineItem[] | null | undefined>(undefined);
  useEffect(() => {
    let current = true;
    pdf.getOutline().then(
      (items) => current && setOutline(items as OutlineItem[] | null),
      () => current && setOutline(null),
    );
    return () => {
      current = false;
    };
  }, [pdf]);

  const jump = async (dest: OutlineItem["dest"]) => {
    const explicit = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
    const target = Array.isArray(explicit) ? explicit[0] : null;
    if (target && typeof target === "object") onJump((await pdf.getPageIndex(target as Parameters<typeof pdf.getPageIndex>[0])) + 1);
    else if (typeof target === "number") onJump(target + 1);
  };

  if (outline === undefined) return <p className="p-4 text-xs text-muted">Reading contents…</p>;
  if (!outline?.length) return <p className="p-4 text-xs text-muted">This PDF has no table of contents.</p>;
  const render = (items: OutlineItem[], depth: number) => (
    <ul className={depth === 0 ? "py-2" : ""}>
      {items.map((item, index) => (
        <li key={`${depth}-${index}`}>
          <button
            type="button"
            onClick={() => void jump(item.dest)}
            className="block w-full truncate px-3 py-1 text-left text-[13px] text-fg-soft hover:bg-white/5 hover:text-fg"
            style={{ paddingLeft: 12 + depth * 12 }}
            title={item.title}
          >
            {item.title}
          </button>
          {item.items.length ? render(item.items, depth + 1) : null}
        </li>
      ))}
    </ul>
  );
  return render(outline, 0);
}

function Highlights({ marks, page, onJump }: { marks: ListedMark[]; page: number; onJump: (page: number) => void }) {
  if (marks.length === 0) return <p className="p-4 text-xs text-muted">Select text on a page to highlight it. Highlights collect here.</p>;
  return (
    <ul aria-label="Highlights" className="space-y-1 p-2">
      {[...marks].sort((a, b) => a.page - b.page).map((mark) => (
        <li key={mark.id}>
          <button type="button" onClick={() => onJump(mark.page)} className={`w-full rounded-md border-l-[3px] px-2 py-1.5 text-left hover:bg-white/5 ${mark.page === page ? "bg-white/5" : ""}`} style={{ borderLeftColor: mark.color ? colorCss(mark.color) : "#f59e0b" }}>
            <span className="block text-[10px] uppercase tracking-wider text-subtle">Page {mark.page}</span>
            <span className="line-clamp-3 font-reading text-[13px] text-fg-soft">{mark.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function Details({ pdf, bytes }: { pdf: PDFDocumentProxy; bytes: number }) {
  const [rows, setRows] = useState<Array<[string, string]> | null>(null);
  useEffect(() => {
    let current = true;
    void Promise.all([pdf.getMetadata().catch(() => null), pdf.getPage(1)]).then(([metadata, first]) => {
      if (!current) return;
      const info = (metadata?.info ?? {}) as Record<string, unknown>;
      const text = (key: string) => (typeof info[key] === "string" && (info[key] as string).trim() ? (info[key] as string).trim() : null);
      const viewport = first.getViewport({ scale: 1 });
      const size = `${Math.round((viewport.width / 72) * 25.4)} × ${Math.round((viewport.height / 72) * 25.4)} mm`;
      const date = (raw: string | null) => {
        const match = raw ? /D:(\d{4})(\d{2})(\d{2})/.exec(raw) : null;
        return match ? `${match[1]}-${match[2]}-${match[3]}` : null;
      };
      const all: Array<[string, string | null]> = [
        ["Title", text("Title")],
        ["Author", text("Author")],
        ["Subject", text("Subject")],
        ["Keywords", text("Keywords")],
        ["Pages", String(pdf.numPages)],
        ["Page size", size],
        ["File size", bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`],
        ["Created", date(text("CreationDate"))],
        ["Modified", date(text("ModDate"))],
        ["Made with", text("Creator")],
        ["PDF producer", text("Producer")],
        ["PDF version", text("PDFFormatVersion")],
      ];
      setRows(all.filter((row): row is [string, string] => row[1] !== null));
    });
    return () => {
      current = false;
    };
  }, [pdf, bytes]);
  if (!rows) return <p className="p-4 text-xs text-muted">Reading details…</p>;
  return (
    <dl className="space-y-2.5 p-4 text-[13px]">
      {rows.map(([label, value]) => (
        <div key={label}>
          <dt className="text-[10px] uppercase tracking-wider text-subtle">{label}</dt>
          <dd className="break-words text-fg-soft">{value}</dd>
        </div>
      ))}
    </dl>
  );
}
