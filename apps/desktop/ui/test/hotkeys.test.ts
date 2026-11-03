// HK-02 hotkey rules. Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import { DEFAULT_HOTKEYS, hint, problemText, record, rowStatus, type HotkeysView } from "../src/hotkeys.ts";

const key = (code: string, key: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({
  code,
  key,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

test("hk02 recorder takes the key code and modifiers, waits on modifiers, Esc and Tab cancel", () => {
  assert.deepEqual(record(key("KeyK", "k", { ctrlKey: true, altKey: true })), {
    kind: "pressed",
    pressed: { code: "KeyK", ctrl: true, alt: true, shift: false, meta: false },
    text: "Ctrl+Alt+K",
  });
  // The code, not the character: AltGr+2 on the Swedish layout is "@" but the key is Digit2.
  assert.deepEqual(record(key("Digit2", "@", { ctrlKey: true, altKey: true })).kind, "pressed");
  assert.equal((record(key("F9", "F9", { shiftKey: true, metaKey: true })) as { text: string }).text, "Shift+Win+F9");
  assert.deepEqual(record(key("ControlLeft", "Control", { ctrlKey: true })), { kind: "partial", text: "Ctrl+…" });
  assert.deepEqual(record(key("AltRight", "AltGraph", { ctrlKey: true, altKey: true })), {
    kind: "partial",
    text: "Ctrl+Alt+…",
  });
  assert.deepEqual(record(key("Escape", "Escape")), { kind: "cancel" });
  assert.deepEqual(record(key("Tab", "Tab")), { kind: "cancel" });
  // With modifiers they are keys Rust refuses (with a reason), not a cancel.
  assert.equal(record(key("Escape", "Escape", { ctrlKey: true })).kind, "pressed");
});

test("hk02 refusals say why, naming the action, the layout and the character", () => {
  assert.match(problemText({ kind: "duplicate", action: "show_hide" }, "Ctrl+Alt+L"), /Ctrl\+Alt\+L .*“Show or hide the lens”/);
  const altgr = problemText({ kind: "alt_gr", layout: "sv-SE", character: "@" }, "Ctrl+Alt+2");
  assert.match(altgr, /AltGr\+2 on the Swedish \(Sweden\) keyboard, which types “@”/);
  assert.match(problemText({ kind: "in_use", keys: "Ctrl+Alt+K" }, "Ctrl+Alt+K"), /another app or by Windows/);
  assert.match(problemText({ kind: "no_modifier" }, "Shift+K"), /Ctrl, Alt or the Windows key/);
  assert.match(problemText({ kind: "key" }, "Ctrl+Space"), /^Ctrl\+Space can't be a hotkey: use a letter, a digit or F1–F24/);
  // An unknown layout tag is shown as it is.
  assert.match(problemText({ kind: "alt_gr", layout: "x-unknown-tag-zz", character: "€" }, "Ctrl+Alt+E"), /keyboard/);
});

test("hk02 tooltips name working hotkeys only; inactive ones say why", () => {
  assert.equal(hint(DEFAULT_HOTKEYS, "swap"), " (Ctrl+Alt+S)");
  const v: HotkeysView = {
    rows: [
      { action: "swap", keys: null, default: "Ctrl+Alt+S", active: true },
      { action: "freeze", keys: "Ctrl+Alt+F", default: "Ctrl+Alt+F", active: false },
      { action: "copy", keys: "Ctrl+Shift+F9", default: "Ctrl+Alt+C", active: true },
    ],
  };
  assert.equal(hint(v, "swap"), "");
  assert.equal(hint(v, "freeze"), "");
  assert.equal(hint(v, "copy"), " (Ctrl+Shift+F9)");
  assert.match(rowStatus(v.rows[1]!)!, /another app or Windows already uses Ctrl\+Alt\+F/);
  assert.equal(rowStatus(v.rows[0]!), null);
});
