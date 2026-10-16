import { describe, expect, it } from "vitest";

import { cleanQuote, hits, MAX_HIGHLIGHT_RECTS, MAX_QUOTE_CHARS, rotateRegion, selectionRegions, unrotateRegion } from "./pdfGeometry";

const page = { left: 100, top: 50, width: 400, height: 800 };

describe("pdf geometry", () => {
  it("joins the pieces of each selected line and stores them as page fractions", () => {
    const regions = selectionRegions(
      [
        { left: 140, top: 130, width: 100, height: 16 },
        { left: 241, top: 131, width: 120, height: 15 },
        { left: 120, top: 150, width: 200, height: 16 },
        { left: 600, top: 150, width: 40, height: 16 }, // outside the page
      ],
      page,
    );
    expect(regions).toHaveLength(2);
    expect(regions[0]).toEqual({ x: 0.1, y: 0.1, width: 0.5525, height: 0.02 });
    expect(regions[1]!.x).toBeCloseTo(0.05);
    expect(regions[1]!.width).toBeCloseTo(0.5);
  });

  it("clips to the page and caps the number of boxes", () => {
    const [clipped] = selectionRegions([{ left: 450, top: 40, width: 100, height: 20 }], page);
    expect(clipped).toEqual({ x: 0.875, y: 0, width: 0.125, height: 0.0125 });
    const many = Array.from({ length: MAX_HIGHLIGHT_RECTS + 10 }, (_, index) => ({ left: 110, top: 51 + index * 3, width: 10, height: 2 }));
    expect(selectionRegions(many, page).length).toBeLessThanOrEqual(MAX_HIGHLIGHT_RECTS);
  });

  it("collapses whitespace and limits the quote", () => {
    expect(cleanQuote("  improve\n consistency\t across  ")).toBe("improve consistency across");
    expect(cleanQuote("x".repeat(MAX_QUOTE_CHARS + 5))).toHaveLength(MAX_QUOTE_CHARS);
  });

  it("turns regions with the page and back", () => {
    const region = { x: 0.1, y: 0.2, width: 0.3, height: 0.05 };
    expect(rotateRegion(region, 90)).toEqual({ x: 0.75, y: 0.1, width: 0.05, height: 0.3 });
    expect(rotateRegion(region, 180).x).toBeCloseTo(0.6);
    expect(rotateRegion(region, 180).y).toBeCloseTo(0.75);
    for (const rotation of [0, 90, 180, 270]) {
      const back = unrotateRegion(rotateRegion(region, rotation), rotation);
      expect(back.x).toBeCloseTo(region.x);
      expect(back.y).toBeCloseTo(region.y);
      expect(back.width).toBeCloseTo(region.width);
      expect(back.height).toBeCloseTo(region.height);
    }
  });

  it("finds the box under a point", () => {
    expect(hits([{ x: 0.1, y: 0.1, width: 0.2, height: 0.05 }], 0.2, 0.12)).toBe(true);
    expect(hits([{ x: 0.1, y: 0.1, width: 0.2, height: 0.05 }], 0.5, 0.12)).toBe(false);
  });
});
