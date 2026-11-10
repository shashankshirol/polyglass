// Settings window: appearance (LENS-07), hotkeys (HK-02) and the way to the model manager. Rust
// checks and saves every value (`prefs.rs`, `hotkeys.rs`); this page draws `SettingsView` and
// sends changes as they happen.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_APPEARANCE,
  FONTS,
  blockBackground,
  chromeColors,
  type Appearance,
  type FontId,
} from "./appearance";
import {
  ACTION_NAMES,
  DEFAULT_HOTKEYS,
  problemText,
  record,
  rowStatus,
  type Action,
  type HotkeysView,
  type Problem,
} from "./hotkeys";

/** `prefs::SettingsView`. */
interface SettingsView {
  appearance: Appearance;
  appearance_defaults: Appearance;
  hotkeys: HotkeysView;
}

/** `commands::HotkeyOutcome`. */
interface HotkeyOutcome {
  hotkeys: HotkeysView;
  problem: Problem | null;
}

const $ = <T extends HTMLElement>(id: string): T => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing #${id}`);
  return el as T;
};

const app = $("app");
const frameColor = $<HTMLInputElement>("frame-color");
const opacity = $<HTMLInputElement>("opacity");
const opacityValue = $<HTMLOutputElement>("opacity-value");
const fontEl = $<HTMLSelectElement>("font");
const fontScale = $<HTMLInputElement>("font-scale");
const fontScaleValue = $<HTMLOutputElement>("font-scale-value");
const previewBar = $("preview-bar");
const previewBlock = $("preview-block");
const motion = $("motion");
const hotkeyRows = $("hotkey-rows");
const hotkeysMessage = $("hotkeys-message");

let appearance: Appearance = DEFAULT_APPEARANCE;
let defaults: Appearance = DEFAULT_APPEARANCE;
let hotkeys: HotkeysView = DEFAULT_HOTKEYS;
/** The action whose keys are being recorded (HK-02). */
let recording: Action | null = null;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  if (!isTauri()) return Promise.resolve(undefined);
  return invoke<T>(cmd, args).catch((e: unknown) => {
    console.error(`${cmd} failed`, e);
    return undefined;
  });
}

function drawAppearance(a: Appearance): void {
  appearance = a;
  frameColor.value = a.frame_color;
  opacity.value = String(a.opacity);
  opacityValue.textContent = `${a.opacity} %`;
  opacity.setAttribute("aria-valuetext", `${a.opacity} percent`);
  fontEl.value = a.font;
  fontScale.value = String(a.font_scale);
  fontScaleValue.textContent = `${a.font_scale} %`;
  fontScale.setAttribute("aria-valuetext", `${a.font_scale} percent`);
  const c = chromeColors(a.frame_color);
  previewBar.style.background = c.chrome;
  previewBar.style.color = c.text;
  previewBlock.style.backgroundColor = blockBackground("#f4f6f8", a.opacity) ?? "";
  previewBlock.style.fontFamily = FONTS[a.font].stack;
  previewBlock.style.fontSize = `${Math.round(14 * (a.font_scale / 100))}px`;
}

function changeAppearance(patch: Partial<Appearance>): void {
  const next = { ...appearance, ...patch };
  drawAppearance(next);
  void call<Appearance>("set_appearance", { appearance: next }).then((saved) => {
    if (saved) drawAppearance(saved);
  });
}

function drawMotion(): void {
  // LENS-07: WebView2 reports Windows' "Animation effects" switch as prefers-reduced-motion.
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  motion.textContent = reduced
    ? "Windows asks for reduced motion (Animation effects are off), so Polyglass draws no animations."
    : "Polyglass draws no animations while Windows' Animation effects are off (Settings → Accessibility → Visual effects).";
}

function hotkeyMessage(text: string | null, ok = false): void {
  hotkeysMessage.hidden = text === null;
  hotkeysMessage.textContent = text ?? "";
  hotkeysMessage.classList.toggle("ok", ok);
}

function button(text: string, label: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = text;
  b.setAttribute("aria-label", label);
  b.addEventListener("click", onClick);
  return b;
}

/** HK-02: one row per action with its keys (a button that records new ones), Default and Off. */
function drawHotkeys(v: HotkeysView): void {
  hotkeys = v;
  const focused = document.activeElement?.id;
  hotkeyRows.replaceChildren(
    ...v.rows.map((row) => {
      const name = ACTION_NAMES[row.action];
      const tr = document.createElement("tr");
      const th = document.createElement("th");
      th.scope = "row";
      th.textContent = name;
      const status = rowStatus(row);
      if (status) {
        const s = document.createElement("span");
        s.className = "status";
        s.textContent = status;
        th.append(s);
      }
      const keysText = row.keys ?? "Off";
      const keys = button(
        keysText,
        `${name}: ${keysText}${status ? `. ${status.replace(/\.$/, "")}` : ""}. Press to change`,
        () => startRecording(row.action, keys),
      );
      keys.id = `hk-${row.action}`;
      keys.className = "keys";
      keys.classList.toggle("off", row.keys === null);
      keys.addEventListener("keydown", (e) => onRecorderKey(e, row.action));
      keys.addEventListener("blur", () => {
        if (recording === row.action) stopRecording();
      });
      const reset = button("Default", `${name}: back to ${row.default}`, () =>
        void bind(row.action, { kind: "default" }, row.default),
      );
      reset.id = `hk-${row.action}-default`;
      reset.disabled = row.keys === row.default && row.active;
      const off = button("Turn off", `${name}: turn off`, () => void bind(row.action, { kind: "off" }, "Off"));
      off.id = `hk-${row.action}-off`;
      off.disabled = row.keys === null;
      const cells = [keys, reset, off].map((b) => {
        const td = document.createElement("td");
        td.append(b);
        return td;
      });
      tr.append(th, ...cells);
      return tr;
    }),
  );
  // Redrawing replaced the buttons: keep the keyboard where it was.
  if (focused?.startsWith("hk-")) {
    const again = document.getElementById(focused) as HTMLButtonElement | null;
    (again && !again.disabled ? again : document.getElementById(focused.replace(/-(default|off)$/, "")))?.focus();
  }
}

function startRecording(action: Action, el: HTMLButtonElement): void {
  if (recording !== null) stopRecording();
  recording = action;
  hotkeyMessage(null);
  // Changed in place: a redraw would replace the focused button, and its blur would cancel.
  el.textContent = "Press keys…";
  el.setAttribute("aria-label", `${ACTION_NAMES[action]}: press the new keys, or Esc to cancel`);
  el.classList.add("recording");
  // Rust lets every combination through while this runs, the hotkeys included.
  void call<HotkeysView>("record_hotkeys", { on: true });
}

function stopRecording(): void {
  if (recording === null) return;
  recording = null;
  drawHotkeys(hotkeys);
  void call<HotkeysView>("record_hotkeys", { on: false }).then((v) => v && drawHotkeys(v));
}

function onRecorderKey(e: KeyboardEvent, action: Action): void {
  if (recording !== action) return;
  const r = record(e);
  if (r.kind === "cancel") {
    if (e.key === "Escape") e.preventDefault();
    stopRecording();
    return;
  }
  e.preventDefault();
  const el = e.currentTarget as HTMLButtonElement;
  if (r.kind === "partial") {
    el.textContent = r.text;
    return;
  }
  recording = null;
  void bind(action, { kind: "pressed", ...r.pressed }, r.text);
}

async function bind(action: Action, binding: Record<string, unknown>, text: string): Promise<void> {
  const out = await call<HotkeyOutcome>("set_hotkey", { action, binding });
  if (!out) {
    drawHotkeys(hotkeys);
    return;
  }
  drawHotkeys(out.hotkeys);
  hotkeyMessage(
    out.problem
      ? problemText(out.problem, text)
      : `${ACTION_NAMES[action]}: ${out.hotkeys.rows.find((r) => r.action === action)?.keys ?? "off"}.`,
    out.problem === null,
  );
}

fontEl.replaceChildren(
  ...(Object.entries(FONTS) as [FontId, { name: string; stack: string }][]).map(([id, f]) => {
    const opt = document.createElement("option");
    opt.value = id;
    opt.textContent = f.name;
    opt.style.fontFamily = f.stack;
    return opt;
  }),
);

frameColor.addEventListener("change", () => changeAppearance({ frame_color: frameColor.value.toLowerCase() }));
opacity.addEventListener("input", () => changeAppearance({ opacity: Number(opacity.value) }));
fontEl.addEventListener("change", () => changeAppearance({ font: fontEl.value as FontId }));
fontScale.addEventListener("input", () => changeAppearance({ font_scale: Number(fontScale.value) }));
$("appearance-reset").addEventListener("click", () => changeAppearance(defaults));
$("open-models").addEventListener("click", () => void call("open_model_manager"));
$("open-privacy").addEventListener("click", () => void call("open_privacy_policy"));
matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", drawMotion);

async function start(): Promise<void> {
  drawMotion();
  const v = await call<SettingsView>("settings_state");
  if (v) {
    defaults = v.appearance_defaults;
    drawAppearance(v.appearance);
    drawHotkeys(v.hotkeys);
    await listen<HotkeysView>("hotkeys-changed", (e) => {
      if (recording === null) drawHotkeys(e.payload);
    });
  } else {
    // Plain browser (layout preview).
    drawAppearance(DEFAULT_APPEARANCE);
    drawHotkeys(DEFAULT_HOTKEYS);
  }
  app.removeAttribute("aria-busy");
}

void start();
