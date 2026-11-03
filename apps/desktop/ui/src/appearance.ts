// LENS-07 appearance rules, kept free of the DOM so `npm test` can run them in Node. Rust
// (`appearance.rs`) validates and saves the values; this file turns them into CSS.

/** `appearance::Appearance`. */
export interface Appearance {
  /** Frame, toolbar and Panel strip colour, `#rrggbb`. */
  frame_color: string;
  /** Opacity of the translated blocks' backgrounds, 20–100 %. */
  opacity: number;
  font: FontId;
  /** 75–200 %. */
  font_scale: number;
}

export type FontId =
  | "system"
  | "arial"
  | "calibri"
  | "verdana"
  | "tahoma"
  | "georgia"
  | "times-new-roman"
  | "consolas";

/** The fonts the Settings window offers (all ship with Windows), with their CSS stacks. */
export const FONTS: Record<FontId, { name: string; stack: string }> = {
  system: { name: "System (Segoe UI)", stack: '"Segoe UI", system-ui, sans-serif' },
  arial: { name: "Arial", stack: "Arial, sans-serif" },
  calibri: { name: "Calibri", stack: "Calibri, sans-serif" },
  verdana: { name: "Verdana", stack: "Verdana, sans-serif" },
  tahoma: { name: "Tahoma", stack: "Tahoma, sans-serif" },
  georgia: { name: "Georgia", stack: "Georgia, serif" },
  "times-new-roman": { name: "Times New Roman", stack: '"Times New Roman", serif' },
  consolas: { name: "Consolas", stack: "Consolas, monospace" },
};

export const DEFAULT_APPEARANCE: Appearance = {
  frame_color: "#182c42",
  opacity: 100,
  font: "system",
  font_scale: 100,
};

/** The frame's alpha (`style.css` `--chrome`). */
const FRAME_ALPHA = 0.92;

/**
 * Toolbar colours for a light-text (dark frame) and a dark-text (light frame) toolbar: the
 * stylesheet's own first, then plain white or black when those don't reach WCAG AA on the frame.
 * Icons and states (accent, on-device green, warning yellow) need 3:1.
 */
const ON_DARK = { text: ["#f2f6fa", "#ffffff"], accent: "#7fd3ff", ok: "#8be08b", warn: "#ffd166", focus: "#ffd34d" };
const ON_LIGHT = { text: ["#111820", "#000000"], accent: "#0b5a8a", ok: "#1e6b34", warn: "#7a4f00", focus: "#8a5a00" };

function rgb(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) return null;
  return [parseInt(m[1]!, 16), parseInt(m[2]!, 16), parseInt(m[3]!, 16)];
}

/** WCAG 2.1 relative luminance of `#rrggbb`. */
export function luminance(hex: string): number {
  const c = rgb(hex) ?? [0, 0, 0];
  const lin = c.map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0]! + 0.7152 * lin[1]! + 0.0722 * lin[2]!;
}

/** WCAG 2.1 contrast ratio between two `#rrggbb` colours. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

export interface ChromeColors {
  /** The frame colour with the frame's alpha. */
  chrome: string;
  /** Toolbar text, ≥ 4.5:1 on the frame colour. */
  text: string;
  /** Pressed buttons, the on-device lock, warnings and the focus ring, ≥ 3:1 on the frame colour. */
  accent: string;
  ok: string;
  warn: string;
  focus: string;
}

/**
 * The toolbar's colours for a frame colour: light text on a dark frame, dark text on a light
 * one, whichever side has more contrast. The contrast is against the opaque frame colour; the
 * desktop shows through its 8 % transparency a little.
 */
export function chromeColors(frame: string): ChromeColors {
  const c = rgb(frame) ?? rgb(DEFAULT_APPEARANCE.frame_color)!;
  const hex = `#${c.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  const side = contrast(hex, "#ffffff") >= contrast(hex, "#000000") ? ON_DARK : ON_LIGHT;
  const text = side.text.find((t) => contrast(hex, t) >= 4.5) ?? side.text[1]!;
  const icon = (color: string): string => (contrast(hex, color) >= 3 ? color : text);
  return {
    chrome: `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${FRAME_ALPHA})`,
    text,
    accent: icon(side.accent),
    ok: icon(side.ok),
    warn: icon(side.warn),
    focus: icon(side.focus),
  };
}

/** A block's background (`#rrggbb` from the pipeline) at the overlay opacity. */
export function blockBackground(bg: string, opacityPercent: number): string | null {
  const c = rgb(bg);
  if (!c) return null;
  const a = Math.min(100, Math.max(20, opacityPercent)) / 100;
  return a >= 1 ? bg : `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${a})`;
}

/** CSS custom properties the lens stylesheet reads. */
export function cssVariables(a: Appearance): Record<string, string> {
  const c = chromeColors(a.frame_color);
  return {
    "--chrome": c.chrome,
    "--chrome-fg": c.text,
    "--accent": c.accent,
    "--chrome-ok": c.ok,
    "--chrome-warn": c.warn,
    "--focus": c.focus,
    "--text-font": (FONTS[a.font] ?? FONTS.system).stack,
    "--font-scale": String(a.font_scale / 100),
  };
}
