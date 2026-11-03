// Hotkey rules for the Settings window and the lens's tooltips (HK-01, HK-02), kept free of the
// DOM so `npm test` can run them in Node. Rust (`hotkeys.rs`) checks and registers every binding.

export type Action = "show_hide" | "freeze" | "copy" | "swap" | "click_through";

/** `hotkeys::HotkeyRow`. */
export interface HotkeyRow {
  action: Action;
  /** `null`: turned off. */
  keys: string | null;
  default: string;
  /** False when another app or Windows holds the keys, so the hotkey doesn't work. */
  active: boolean;
}

/** `hotkeys::HotkeysView`. */
export interface HotkeysView {
  rows: HotkeyRow[];
}

/** `hotkeys::Problem`: why a binding was refused. */
export type Problem =
  | { kind: "key" }
  | { kind: "no_modifier" }
  | { kind: "duplicate"; action: Action }
  | { kind: "alt_gr"; layout: string; character: string }
  | { kind: "in_use"; keys: string };

/** `hotkeys::Pressed`, sent as `{ kind: "pressed", ... }`. */
export interface Pressed {
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export const ACTION_NAMES: Record<Action, string> = {
  show_hide: "Show or hide the lens",
  freeze: "Freeze the screen",
  copy: "Copy the translation",
  swap: "Swap languages",
  click_through: "Click-through on or off",
};

export const DEFAULT_HOTKEYS: HotkeysView = {
  rows: (
    [
      ["show_hide", "L"],
      ["freeze", "F"],
      ["copy", "C"],
      ["swap", "S"],
      ["click_through", "T"],
    ] as const
  ).map(([action, key]) => ({ action, keys: `Ctrl+Alt+${key}`, default: `Ctrl+Alt+${key}`, active: true })),
};

/** The keys of a hotkey that works, or `null` when it is off or another app holds them. */
export function activeKeys(view: HotkeysView, action: Action): string | null {
  const row = view.rows.find((r) => r.action === action);
  return row?.keys && row.active ? row.keys : null;
}

/** " (Ctrl+Alt+S)" for a tooltip, or "". */
export function hint(view: HotkeysView, action: Action): string {
  const keys = activeKeys(view, action);
  return keys ? ` (${keys})` : "";
}

function languageName(tag: string): string {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" }).of(tag) ?? tag;
  } catch {
    return tag;
  }
}

/** The sentence the Settings window shows when `keys` were refused. */
export function problemText(p: Problem, keys: string): string {
  switch (p.kind) {
    case "key":
      return `${keys} can't be a hotkey: use a letter, a digit or F1–F24, with Ctrl, Alt or the Windows key.`;
    case "no_modifier":
      return `${keys} needs Ctrl, Alt or the Windows key too: on its own it would stop working in every other app.`;
    case "duplicate":
      return `${keys} is already the hotkey for “${ACTION_NAMES[p.action]}”.`;
    case "alt_gr": {
      const key = keys.split("+").pop() ?? "";
      return `${keys} is AltGr+${key} on the ${languageName(p.layout)} keyboard, which types “${p.character}”. As a hotkey it would stop “${p.character}” from working in every app. Choose another key.`;
    }
    case "in_use":
      return `${p.keys} is already used by another app or by Windows. Choose another one.`;
  }
}

/** The row's status line: why a saved hotkey doesn't work. */
export function rowStatus(row: HotkeyRow): string | null {
  if (row.keys && !row.active) {
    return `Not working: another app or Windows already uses ${row.keys}.`;
  }
  return null;
}

/** What a key press means to the recorder. */
export type Recorded =
  | { kind: "cancel" }
  /** Only modifiers so far: show them and wait. */
  | { kind: "partial"; text: string }
  | { kind: "pressed"; pressed: Pressed; text: string };

const MODIFIER_KEYS = new Set(["Control", "Alt", "AltGraph", "Shift", "Meta", "OS"]);

/** Pick the keys from a `keydown` while the recorder runs. Esc and Tab cancel. */
export function record(e: {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): Recorded {
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(
    Boolean,
  ) as string[];
  if ((e.key === "Escape" || e.key === "Tab") && mods.length === 0) return { kind: "cancel" };
  if (MODIFIER_KEYS.has(e.key)) return { kind: "partial", text: mods.length ? `${mods.join("+")}+…` : "…" };
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  return {
    kind: "pressed",
    pressed: { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey },
    text: [...mods, key].join("+"),
  };
}
