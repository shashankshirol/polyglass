// Settings window: appearance (LENS-07) and the way to the model manager. Rust checks and saves
// every value (`prefs.rs`); this page draws `SettingsView` and sends changes as they happen.

import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  DEFAULT_APPEARANCE,
  FONTS,
  blockBackground,
  chromeColors,
  type Appearance,
  type FontId,
} from "./appearance";

/** `prefs::SettingsView`. */
interface SettingsView {
  appearance: Appearance;
  appearance_defaults: Appearance;
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

let appearance: Appearance = DEFAULT_APPEARANCE;
let defaults: Appearance = DEFAULT_APPEARANCE;

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
matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", drawMotion);

async function start(): Promise<void> {
  drawMotion();
  const v = await call<SettingsView>("settings_state");
  if (v) {
    defaults = v.appearance_defaults;
    drawAppearance(v.appearance);
  } else {
    // Plain browser (layout preview).
    drawAppearance(DEFAULT_APPEARANCE);
  }
  app.removeAttribute("aria-busy");
}

void start();
