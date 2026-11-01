// REN-02 font fitting, kept free of the DOM so `npm test` can run it in Node.

/** Smallest font size a translation is drawn at (REN-02). */
export const MIN_FONT_PX = 9;
/** Largest starting size, so a misread giant box doesn't draw huge text. */
export const MAX_FONT_PX = 48;

/**
 * The size to start fitting at: the source text's own size. Windows OCR's line boxes span the
 * glyphs from ascender to descender, which for UI fonts is about one em.
 */
export function startSize(lineHeight: number): number {
  if (!Number.isFinite(lineHeight)) return MIN_FONT_PX;
  return Math.min(MAX_FONT_PX, Math.max(MIN_FONT_PX, Math.round(lineHeight)));
}

export interface Fit {
  /** Font size in px. */
  size: number;
  /** True when the text doesn't fit even at `MIN_FONT_PX`: the box grows downward. */
  overflow: boolean;
}

/**
 * The largest whole-px size from `start` down to `MIN_FONT_PX` at which `fits(size)` holds.
 * `fits` lays the text out at that size and reports whether it stays inside the source box.
 */
export function fitFontSize(fits: (size: number) => boolean, start: number): Fit {
  const top = Math.max(MIN_FONT_PX, Math.floor(start));
  for (let size = top; size >= MIN_FONT_PX; size--) {
    if (fits(size)) return { size, overflow: false };
  }
  return { size: MIN_FONT_PX, overflow: true };
}

/**
 * The height a block starting `top` px down may grow to before it reaches the bottom of the lens
 * interior (`limit` px tall). Text past that is cut; the tooltip has all of it.
 */
export function maxHeight(top: number, limit: number): number {
  return Math.max(0, limit - top);
}
