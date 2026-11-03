// REN-02 font fitting, kept free of the DOM so `npm test` can run it in Node.

/** Smallest font size a translation is drawn at (REN-02), at a font scale of 100 %. */
export const MIN_FONT_PX = 9;
/** Largest starting size at 100 %, so a misread giant box doesn't draw huge text. */
export const MAX_FONT_PX = 48;

/** The smallest size at a font scale (LENS-07: 75–200 %, as a factor). */
export function minSize(scale = 1): number {
  return Math.round(MIN_FONT_PX * scale);
}

/**
 * The size to start fitting at: the source text's own size, times the font scale (LENS-07).
 * Windows OCR's line boxes span the glyphs from ascender to descender, which for UI fonts is
 * about one em.
 */
export function startSize(lineHeight: number, scale = 1): number {
  const min = minSize(scale);
  if (!Number.isFinite(lineHeight)) return min;
  return Math.min(Math.round(MAX_FONT_PX * scale), Math.max(min, Math.round(lineHeight * scale)));
}

export interface Fit {
  /** Font size in px. */
  size: number;
  /** True when the text doesn't fit even at `MIN_FONT_PX`: the box grows downward. */
  overflow: boolean;
}

/**
 * The largest whole-px size from `start` down to `min` (`MIN_FONT_PX` at 100 %) at which
 * `fits(size)` holds. `fits` lays the text out at that size and reports whether it stays inside
 * the source box.
 */
export function fitFontSize(fits: (size: number) => boolean, start: number, min = MIN_FONT_PX): Fit {
  const top = Math.max(min, Math.floor(start));
  for (let size = top; size >= min; size--) {
    if (fits(size)) return { size, overflow: false };
  }
  return { size: min, overflow: true };
}

/**
 * The height a block starting `top` px down may grow to before it reaches the bottom of the lens
 * interior (`limit` px tall). Text past that is cut; the tooltip has all of it.
 */
export function maxHeight(top: number, limit: number): number {
  return Math.max(0, limit - top);
}

export interface BlockFit extends Fit {
  /** Words may break at the box edge (SV-06): set only when no size fits without it. */
  breakWords: boolean;
}

/**
 * SV-06 on top of REN-02: shrink first with whole words (the webview may still hyphenate them,
 * the blocks carry `lang`), so a long Swedish compound gets a smaller font rather than a break in
 * the middle. Only when no size down to `min` fits are words broken at the box edge, which keeps
 * them inside the box's width; text that still doesn't fit grows the box downward.
 */
export function fitBlock(
  fits: (size: number, breakWords: boolean) => boolean,
  start: number,
  min = MIN_FONT_PX,
): BlockFit {
  const whole = fitFontSize((px) => fits(px, false), start, min);
  if (!whole.overflow) return { ...whole, breakWords: false };
  return { ...fitFontSize((px) => fits(px, true), start, min), breakWords: true };
}
