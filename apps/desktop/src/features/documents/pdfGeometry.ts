import type { HighlightColor, Region } from "../../services/library";

/** Most line boxes and characters a highlight keeps; the backend checks the same limits. */
export const MAX_HIGHLIGHT_RECTS = 256;
export const MAX_QUOTE_CHARS = 4000;

export const HIGHLIGHT_COLORS: Array<{ value: HighlightColor; label: string; css: string }> = [
  { value: "yellow", label: "Yellow", css: "#facc15" },
  { value: "green", label: "Green", css: "#4ade80" },
  { value: "blue", label: "Blue", css: "#60a5fa" },
  { value: "pink", label: "Pink", css: "#f472b6" },
  { value: "orange", label: "Orange", css: "#fb923c" },
  { value: "purple", label: "Purple", css: "#c084fc" },
];

export const colorCss = (color: HighlightColor) => HIGHLIGHT_COLORS.find((item) => item.value === color)?.css ?? "#facc15";

interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Turns the client rectangles of a text selection into a few line boxes, in
 * fractions of the page: boxes outside the page are dropped, the rest are
 * clipped to it, and boxes on the same line that touch are joined.
 */
export function selectionRegions(rects: Box[], page: Box): Region[] {
  const clipped = rects
    .map((rect) => {
      const left = Math.max(rect.left, page.left);
      const top = Math.max(rect.top, page.top);
      const right = Math.min(rect.left + rect.width, page.left + page.width);
      const bottom = Math.min(rect.top + rect.height, page.top + page.height);
      return { left, top, right, bottom };
    })
    .filter((rect) => rect.right - rect.left > 0.5 && rect.bottom - rect.top > 0.5)
    .sort((a, b) => a.top - b.top || a.left - b.left);

  const lines: Array<{ left: number; top: number; right: number; bottom: number }> = [];
  for (const rect of clipped) {
    const line = lines.find((candidate) => {
      const overlap = Math.min(candidate.bottom, rect.bottom) - Math.max(candidate.top, rect.top);
      const sameLine = overlap > 0.5 * Math.min(candidate.bottom - candidate.top, rect.bottom - rect.top);
      const touching = rect.left <= candidate.right + 4 && rect.right >= candidate.left - 4;
      return sameLine && touching;
    });
    if (line) {
      line.left = Math.min(line.left, rect.left);
      line.right = Math.max(line.right, rect.right);
      line.top = Math.min(line.top, rect.top);
      line.bottom = Math.max(line.bottom, rect.bottom);
    } else {
      lines.push({ ...rect });
    }
  }

  const fraction = (value: number) => Math.min(1, Math.max(0, value));
  return lines.slice(0, MAX_HIGHLIGHT_RECTS).map((line) => {
    const x = fraction((line.left - page.left) / page.width);
    const y = fraction((line.top - page.top) / page.height);
    return {
      x,
      y,
      width: Math.min(1 - x, (line.right - line.left) / page.width),
      height: Math.min(1 - y, (line.bottom - line.top) / page.height),
    };
  });
}

/** The selected text with whitespace collapsed, cut to the length a highlight keeps. */
export function cleanQuote(text: string): string {
  return text.replace(/\s+/g, " ").trim().slice(0, MAX_QUOTE_CHARS);
}

/** Whether a point, in page fractions, falls inside any of the boxes. */
export function hits(rects: Region[], x: number, y: number): boolean {
  return rects.some((rect) => x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height);
}

/** A note body that quotes the highlighted text, so the quote stays with the note. */
export function quoteBody(quote: string): string {
  return JSON.stringify({ type: "doc", content: [{ type: "blockquote", content: [{ type: "paragraph", content: [{ type: "text", text: quote }] }] }, { type: "paragraph" }] });
}

/**
 * A region of the unrotated page as it appears on a page turned clockwise by
 * `rotation` degrees (0, 90, 180, or 270). Highlights are stored unrotated.
 */
export function rotateRegion(region: Region, rotation: number): Region {
  const { x, y, width, height } = region;
  switch (((rotation % 360) + 360) % 360) {
    case 90:
      return { x: 1 - (y + height), y: x, width: height, height: width };
    case 180:
      return { x: 1 - (x + width), y: 1 - (y + height), width, height };
    case 270:
      return { x: y, y: 1 - (x + width), width: height, height: width };
    default:
      return region;
  }
}

/** The inverse of {@link rotateRegion}: a region seen on the turned page, on the unrotated page. */
export function unrotateRegion(region: Region, rotation: number): Region {
  return rotateRegion(region, 360 - (((rotation % 360) + 360) % 360));
}
