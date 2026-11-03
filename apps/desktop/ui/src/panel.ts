// Panel mode (MODE-02): the interior stays transparent and the translations are listed in a strip
// attached to the lens, below it or above it when there is no room below (Rust decides,
// `lens::panel_side`). Pure functions, so `npm test` covers them.

import type { RenderedBlock } from "./types.ts";

/** `lens::Strip`: where the window has the panel strip; "none" is Overlay mode. */
export type Strip = "none" | "below" | "above";
/** `lens::DisplayMode`, as saved in `settings.toml`. */
export type DisplayMode = "overlay" | "panel";

export interface PanelItem {
  text: string;
  lang: string;
  dim: boolean;
  /** Shown as it is: already in the target language (TR-07), or the original (REN-04). */
  untranslated: boolean;
}

/**
 * One entry per block, in the order the pipeline sends them: the reading order of the source
 * (grouping), not the order of their boxes on screen. Holding left Alt shows the originals
 * (REN-04).
 */
export function panelItems(blocks: readonly RenderedBlock[], showOriginal: boolean): PanelItem[] {
  return blocks.map((b) => {
    const translated = !showOriginal && b.translation !== null && !b.passthrough;
    return {
      text: translated ? (b.translation ?? b.source) : b.source,
      lang: translated ? b.target_lang : b.source_lang,
      dim: b.dim,
      untranslated: !translated,
    };
  });
}

export function modeOf(strip: Strip): DisplayMode {
  return strip === "none" ? "overlay" : "panel";
}

/** The toolbar's mode button: its text, the mode a click switches to, and its accessible name. */
export function modeButton(strip: Strip): { text: string; next: DisplayMode; label: string } {
  const panel = modeOf(strip) === "panel";
  return {
    text: panel ? "Panel" : "Overlay",
    next: panel ? "overlay" : "panel",
    label: panel
      ? `Mode: Panel, translations listed ${strip} the lens. Switch to Overlay`
      : "Mode: Overlay, translations drawn over the text. Switch to Panel",
  };
}
