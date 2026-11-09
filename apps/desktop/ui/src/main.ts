// Lens UI: a thin renderer for `LensUpdate` events plus the toolbar (PLAN §3.3). All state and
// validation live in Rust; this file only draws and forwards user input to commands.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_APPEARANCE, blockBackground, cssVariables, type Appearance } from "./appearance";
import { MODEL_MESSAGES, formatBytes, percent } from "./catalog";
import { fitBlock, maxHeight, minSize, startSize } from "./fit";
import { DEFAULT_HOTKEYS, activeKeys, hint, type HotkeysView } from "./hotkeys";
import {
  langName,
  modelMessage,
  ocrHelp,
  parseSourceValue,
  pickSource,
  primary,
  sourceLabel,
  sourceOptions,
  sourceValue,
  targetOptions,
  type PickerOption,
} from "./langs";
import { modeButton, panelItems, type Strip } from "./panel";
import type { LanguagesView, LensConfig, LensUpdate, ModelsView, RenderedBlock } from "./types";

type ResizeDirection =
  | "North"
  | "South"
  | "East"
  | "West"
  | "NorthEast"
  | "NorthWest"
  | "SouthEast"
  | "SouthWest";

const $ = <T extends HTMLElement>(id: string): T => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing #${id}`);
  return el as T;
};

const lensEl = $("lens");
const interiorEl = $("interior");
const blocksEl = $("blocks");
const panelEl = $("panel");
const panelList = $("panel-list");
const modeBtn = $<HTMLButtonElement>("mode");
const freezeBtn = $<HTMLButtonElement>("freeze");
const messageEl = $("message");
const noticeEl = $("notice");
const privacyEl = $("privacy");
const privacyName = $("privacy-name");
const lockIcon = document.getElementById("icon-lock") as unknown as SVGElement;
const cloudIcon = document.getElementById("icon-cloud") as unknown as SVGElement;
const pauseBtn = $<HTMLButtonElement>("pause");
const copyBtn = $<HTMLButtonElement>("copy");
const swapBtn = $<HTMLButtonElement>("swap");
const sourceEl = $<HTMLSelectElement>("source-lang");
const targetEl = $<HTMLSelectElement>("target-lang");
const clickBtn = $<HTMLButtonElement>("click-through");
const ocrLangBtn = $<HTMLButtonElement>("ocr-lang");
const modelsBtn = $<HTMLButtonElement>("models");
const settingsBtn = $<HTMLButtonElement>("settings");
const badge = $("mode-badge");
const traceBadge = $("trace-badge");
const debugBanner = $("debug-banner");

const HEX = /^#[0-9a-fA-F]{6}$/;
let lastGeneration = -1;
let paused = false;
/** The update on screen, redrawn when left Alt is pressed or released (REN-04). */
let current: LensUpdate | null = null;
/** True while left Alt is held: show the source text instead of the translation (REN-04). */
let showOriginal = false;
let mode: LensConfig["mode"] = "none";
/** Where the Panel mode strip is; "none" in Overlay mode (MODE-02). */
let strip: Strip = "none";
/** Freeze mode is on (MODE-03): the lens reads a snapshot of the monitor. */
let frozen = false;
/** Progress text while the model manager downloads. */
let downloadProgress: string | null = null;
/** Whether the missing models can be downloaded (false: import-only, see README). */
let modelsDownloadable = true;
/** Set when a download found the models release unreachable (e.g. a private repository). */
let modelsUnreachable = false;
/** The last model download or delete outcome already shown. */
let modelsMessage: string | null = null;
/** Frame colour, opacity, font and font scale (LENS-07). */
let appearance: Appearance = DEFAULT_APPEARANCE;
/** The hotkeys named in the tooltips (HK-02: they can be rebound or turned off). */
let hotkeys: HotkeysView = DEFAULT_HOTKEYS;
/** `--trace-timings`: tell Rust when each update has been painted. */
let traceTimings = false;
/** The language pair and what the pickers offer (MDL-07). */
let languages: LanguagesView = {
  source: "sv",
  target: "en",
  auto: true,
  pairs: [{ source: "sv", target: "en", installed: true }],
  canSwap: false,
};
/** Bytes of the selected pair's models still missing, and how to install them by hand. */
let missingBytes = 0;
let installCommand = "python scripts/models/opus_mt.py";

// Message IDs from the pipeline and their text. The UI is in English in v1 (D-17).
const MESSAGES: Record<string, string> = {
  "no-text": "No text found",
  "cap-error": "Screen capture failed",
  "cap-error-permission": "Screen capture permission is missing",
  "ocr-error": "Text recognition failed",
  "ocr-error-language-missing": "Text recognition language is not installed",
  "cap-error-self-exclusion":
    "The lens can't hide itself from screen capture (needs Windows 10 version 2004 or later), so live capture is off.",
  "tr-error-backend": "Translation failed",
  ...MODEL_MESSAGES,
};

/** Messages that name the selected pair; the rest are in MESSAGES. */
function message(id: string): string {
  switch (id) {
    case "tr-error-model-missing":
      return modelMessage(languages, "download", missingBytes, installCommand);
    // Import-only manifest entries (no download URLs): installed from the repository.
    case "tr-error-model-import":
      return modelMessage(languages, "import", missingBytes, installCommand);
    // The lens has room for the whole fallback, so it carries the command too.
    case "mdl-error-unreachable":
      return modelMessage(languages, "unreachable", missingBytes, installCommand);
    default:
      return MESSAGES[id] ?? id;
  }
}

function showMessage(text: string | null): void {
  messageEl.hidden = text === null;
  messageEl.textContent = text ?? "";
}

function setPrivacy(local: boolean, name: string): void {
  // PRIV-05: lock for on-device, cloud + provider name for online backends.
  lockIcon.toggleAttribute("hidden", !local);
  cloudIcon.toggleAttribute("hidden", local);
  privacyName.textContent = local ? "" : name;
  const label = local ? "On-device translation" : `Online translation: ${name}`;
  privacyEl.setAttribute("aria-label", label);
  privacyEl.title = label;
  privacyEl.classList.toggle("online", !local);
}

function renderBlock(b: RenderedBlock): HTMLElement {
  const el = document.createElement("div");
  el.className = "block";
  el.classList.toggle("dim", b.dim);
  // textContent only: recognized text is never parsed as HTML.
  const translated = !showOriginal && b.translation !== null && !b.passthrough;
  el.textContent = showOriginal ? b.source : (b.translation ?? b.source);
  el.lang = translated ? b.target_lang : b.source_lang;
  el.style.left = `${b.rect.x}px`;
  el.style.top = `${b.rect.y}px`;
  el.style.width = `${b.rect.w}px`;
  el.style.minHeight = `${b.rect.h}px`;
  // LENS-07: the overlay opacity applies to the background; the text stays opaque.
  const bg = HEX.test(b.bg) ? blockBackground(b.bg, appearance.opacity) : null;
  if (bg) el.style.backgroundColor = bg;
  if (HEX.test(b.fg)) el.style.color = b.fg;
  return el;
}

/**
 * REN-02: start at the source text's size and shrink to fit the source box, down to 9 px. Text
 * that still doesn't fit grows the box downward to the interior's bottom edge, and the block's
 * tooltip holds the full text (seen with click-through off), and Panel mode lists it in full.
 * SV-06: words break at the box edge only when no size fits with whole (or hyphenated) words.
 */
function fit(el: HTMLElement, b: RenderedBlock): void {
  const { rect } = b;
  const scale = appearance.font_scale / 100;
  const { size, overflow, breakWords } = fitBlock(
    (px, breaking) => {
      el.style.fontSize = `${px}px`;
      el.classList.toggle("break-words", breaking);
      return el.scrollHeight <= rect.h + 1 && el.scrollWidth <= rect.w + 1;
    },
    startSize(b.line_height, scale),
    minSize(scale),
  );
  el.style.fontSize = `${size}px`;
  el.classList.toggle("break-words", breakWords);
  el.classList.toggle("overflow", overflow);
  if (overflow) {
    el.style.maxHeight = `${maxHeight(rect.y, blocksEl.clientHeight)}px`;
    el.title = el.textContent ?? "";
  }
}

function drawBlocks(u: LensUpdate): void {
  if (strip !== "none") {
    // MODE-02: the interior stays transparent; the strip lists the blocks in reading order.
    blocksEl.replaceChildren();
    drawPanel(u.blocks);
    return;
  }
  panelList.replaceChildren();
  const els = u.blocks.map(renderBlock);
  blocksEl.replaceChildren(...els);
  els.forEach((el, i) => fit(el, u.blocks[i]!));
}

function drawPanel(blocks: readonly RenderedBlock[]): void {
  panelList.replaceChildren(
    ...panelItems(blocks, showOriginal).map((item) => {
      const li = document.createElement("li");
      // textContent only: recognized text is never parsed as HTML.
      li.textContent = item.text;
      li.lang = item.lang;
      li.classList.toggle("dim", item.dim);
      li.classList.toggle("untranslated", item.untranslated);
      return li;
    }),
  );
}

/** MODE-02: Rust laid the window out with the strip below or above the lens, or without it. */
function setStrip(s: Strip): void {
  strip = s;
  document.body.classList.toggle("strip-below", s === "below");
  document.body.classList.toggle("strip-above", s === "above");
  panelEl.hidden = s === "none";
  // Messages and notices go where the translations are, so the interior stays clear.
  if (s === "none") interiorEl.append(messageEl, noticeEl);
  else panelEl.append(messageEl, noticeEl);
  const button = modeButton(s);
  modeBtn.textContent = button.text;
  modeBtn.title = button.label;
  modeBtn.setAttribute("aria-label", button.label);
  if (current && !paused) drawBlocks(current);
  else if (paused) panelList.replaceChildren();
}

/** REN-04: the Rust side reports left Alt (alone) being held or released. */
function setShowOriginal(on: boolean): void {
  showOriginal = on;
  lensEl.classList.toggle("original", on);
  if (current && !paused) drawBlocks(current);
}

function render(u: LensUpdate): void {
  // The driver never emits stale generations (CAP-03); this guards against reordering anyway.
  if (u.generation < lastGeneration) return;
  lastGeneration = u.generation;

  setPrivacy(u.backend.local, u.backend.name);
  current = u;
  drawBlocks(u);
  if (traceTimings) reportDrawn(u.generation);

  switch (u.status.kind) {
    case "ok":
      showMessage(null);
      break;
    case "no_text_found":
      showMessage(MESSAGES["no-text"] ?? "No text found");
      break;
    case "error":
    {
      const id =
        u.status.message_id === "tr-error-model-missing" && !modelsDownloadable
          ? modelsUnreachable
            ? "mdl-error-unreachable"
            : "tr-error-model-import"
          : u.status.message_id;
      if (id === "tr-error-model-missing") modelsBtn.hidden = mode !== "live";
      showMessage(id === "tr-error-model-missing" && downloadProgress ? downloadProgress : message(id));
      break;
    }
  }
  // Live mode only falls back to the mock if someone wires it; say so when it happens.
  if (mode === "live") setBadge(u.backend.id === "mock" ? "mock" : null);

  // SV-02 for any source language: say which OCR language is missing and where to add it.
  const notice = u.notices.find((n) => n.kind === "ocr_language_missing");
  const failed =
    u.status.kind === "error" && u.status.message_id === "ocr-error-language-missing"
      ? (u.status.lang ?? languages.source)
      : null;
  const missingLang = notice?.lang ?? failed;
  noticeEl.hidden = missingLang === null;
  noticeEl.textContent = missingLang ? ocrHelp(missingLang, languages.target, notice !== undefined) : "";
  ocrLangBtn.hidden = missingLang === null;
  if (missingLang) {
    const label = `${langName(missingLang)} text recognition isn't installed. Open Windows language settings to add it.`;
    ocrLangBtn.textContent = `⚠ ${primary(missingLang).toUpperCase()}`;
    ocrLangBtn.title = label;
    ocrLangBtn.setAttribute("aria-label", label);
  }
}

/**
 * `--trace-timings`: the DOM change above is painted in the next frame; a task queued from that
 * frame's animation callback runs right after the paint.
 */
function reportDrawn(generation: number): void {
  requestAnimationFrame(() => setTimeout(() => call("lens_drawn", { generation }), 0));
}

/** PRIV-02: a persistent red banner while `--debug-dump-frames` saves frames to disk. */
function setDebugFlags(trace: boolean, dumpFrames: string | null): void {
  traceTimings = trace;
  traceBadge.hidden = !trace;
  lensEl.classList.toggle("dumping", dumpFrames !== null);
  debugBanner.hidden = dumpFrames === null;
  debugBanner.textContent =
    dumpFrames === null ? "" : `Debug: saving every captured frame to ${dumpFrames}`;
  debugBanner.title = debugBanner.textContent;
}

/** LENS-07: redraw with a new frame colour, opacity, font or font scale. */
function setAppearance(a: Appearance): void {
  appearance = a;
  const root = document.documentElement.style;
  for (const [name, value] of Object.entries(cssVariables(a))) root.setProperty(name, value);
  if (current && !paused) drawBlocks(current);
}

function applyConfig(cfg: LensConfig): void {
  const root = document.documentElement.style;
  root.setProperty("--frame", `${cfg.layout.frame}px`);
  root.setProperty("--toolbar", `${cfg.layout.toolbar}px`);
  root.setProperty("--panel", `${cfg.layout.panel}px`);
  mode = cfg.mode;
  setBadge(cfg.mode === "demo" ? "demo" : null);
  modelsDownloadable = cfg.modelsDownloadable;
  installCommand = cfg.installCommand;
  setLanguages(cfg.languages);
  setModelsButton(cfg.modelsMissingBytes);
  setClickThroughUi(cfg.clickThrough);
  setStrip(cfg.strip);
  setDebugFlags(cfg.traceTimings, cfg.dumpFrames);
  setAppearance(cfg.appearance);
  setHotkeys(cfg.hotkeys);
  if (cfg.pipelineError) {
    showMessage(message(cfg.pipelineError));
  } else if (!cfg.pipeline) {
    showMessage("Live capture isn't available on this platform yet. Start with --demo for mock results.");
  }
}

function fillPicker(el: HTMLSelectElement, options: PickerOption[], value: string): void {
  el.replaceChildren(
    ...options.map((o) => {
      const opt = document.createElement("option");
      opt.value = o.value;
      opt.textContent = o.text;
      if (o.missing) opt.title = "Model not installed: ⬇ or ⚙ downloads it";
      return opt;
    }),
  );
  el.value = value;
}

/** MDL-07: draw the pickers for the pair in use; Rust sends this after every change. */
function setLanguages(v: LanguagesView): void {
  languages = v;
  fillPicker(sourceEl, sourceOptions(v), sourceValue(v));
  fillPicker(targetEl, targetOptions(v), v.target);
  const source = sourceLabel(v);
  sourceEl.setAttribute("aria-label", source);
  sourceEl.title = source;
  const target = `Target language: ${langName(v.target)}`;
  targetEl.setAttribute("aria-label", target);
  targetEl.title = target;
  swapBtn.disabled = !v.canSwap;
  swapBtn.title = v.canSwap
    ? `Swap languages${hint(hotkeys, "swap")}`
    : `No model for ${langName(v.target)} → ${langName(v.source)}`;
  swapBtn.setAttribute("aria-label", v.canSwap ? "Swap languages" : swapBtn.title);
}

function chooseLanguages(source: string, target: string, auto: boolean): void {
  if (!isTauri()) return;
  invoke<LanguagesView>("set_languages", { source, target, auto })
    .then(setLanguages)
    .catch((e: unknown) => {
      console.error("set_languages failed", e);
      setLanguages(languages);
    });
}

function setBadge(kind: "demo" | "mock" | null): void {
  badge.hidden = kind === null;
  if (kind === "demo") {
    badge.textContent = "DEMO";
    badge.title = "Demo mode: mock capture, OCR and translation";
  } else if (kind === "mock") {
    badge.textContent = "MOCK MT";
    badge.title = "Translation is a placeholder ([en] + original), not the on-device translator.";
  }
  badge.setAttribute("aria-label", badge.title);
}

/** ⬇ opens the model manager while the selected pair's models are missing and downloadable. */
function setModelsButton(bytes: number): void {
  missingBytes = bytes;
  modelsBtn.hidden = !(mode === "live" && bytes > 0 && modelsDownloadable);
  const pair = `${langName(languages.source)} ${languages.canSwap ? "and" : "to"} ${langName(languages.target)}`;
  const label = `Download the ${pair} translation models (${formatBytes(bytes)})`;
  modelsBtn.title = label;
  modelsBtn.setAttribute("aria-label", label);
}

/** The model manager's state changed: mirror its download progress and outcome in the lens. */
function onModelsView(v: ModelsView): void {
  modelsDownloadable = v.wanted_downloadable;
  modelsUnreachable = v.unreachable;
  installCommand = v.install_command;
  if (v.job) {
    const pct = percent(v.job);
    modelsBtn.textContent = `⬇ ${pct}%`;
    downloadProgress = `Downloading the translation models… ${pct}% of ${formatBytes(v.job.total)}`;
    showMessage(downloadProgress);
  } else {
    modelsBtn.textContent = "⬇";
    if (downloadProgress) {
      // The download ended: say how, or clear the progress for the next update.
      showMessage(v.message_id ? message(v.message_id) : null);
    }
    downloadProgress = null;
  }
  if (v.message_id !== modelsMessage && v.message_id === "mdl-error-unreachable") {
    showMessage(message(v.message_id));
  }
  modelsMessage = v.message_id;
  setModelsButton(v.wanted_missing_bytes);
}

function setClickThroughUi(on: boolean): void {
  clickBtn.setAttribute("aria-pressed", String(on));
  lensEl.classList.toggle("interactive", !on);
}

function call(cmd: string, args?: Record<string, unknown>): void {
  if (!isTauri()) return;
  invoke(cmd, args).catch((e: unknown) => console.error(`${cmd} failed`, e));
}

// LENS-02: arrow keys move the lens 1 px, Shift+arrow 10 px.
const ARROWS: Record<string, [number, number]> = {
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
  ArrowUp: [0, -1],
  ArrowDown: [0, 1],
};
window.addEventListener("keydown", (e) => {
  // MODE-03: Esc goes back to the live screen.
  if (e.key === "Escape" && frozen) {
    e.preventDefault();
    call("set_frozen", { frozen: false });
    return;
  }
  const d = ARROWS[e.key];
  if (!d || e.altKey || e.ctrlKey || e.metaKey) return;
  // In the panel the arrow keys scroll it.
  if (panelEl.contains(document.activeElement)) return;
  e.preventDefault();
  const step = e.shiftKey ? 10 : 1;
  call("nudge_lens", { dx: d[0] * step, dy: d[1] * step });
});

// LENS-03: resize from every edge and corner.
for (const el of document.querySelectorAll<HTMLElement>(".rz")) {
  el.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !isTauri()) return;
    e.preventDefault();
    void getCurrentWindow().startResizeDragging(el.dataset.dir as ResizeDirection);
  });
}

pauseBtn.addEventListener("click", () => {
  paused = !paused;
  pauseBtn.setAttribute("aria-pressed", String(paused));
  pauseBtn.setAttribute("aria-label", paused ? "Resume translation" : "Pause translation");
  pauseBtn.title = pauseBtn.getAttribute("aria-label") ?? "";
  pauseBtn.textContent = paused ? "▶" : "❚❚";
  lensEl.classList.toggle("paused", paused);
  if (paused) {
    blocksEl.replaceChildren();
    panelList.replaceChildren();
    current = null;
  }
  call("set_paused", { paused });
});

// MODE-03: ❄ freezes the monitor under the lens, or goes back to the live screen.
freezeBtn.addEventListener("click", () => call("set_frozen", { frozen: !frozen }));

function setFrozen(on: boolean): void {
  frozen = on;
  freezeBtn.setAttribute("aria-pressed", String(on));
  const label = on ? "Unfreeze: back to the live screen (Esc)" : "Freeze the screen";
  freezeBtn.setAttribute("aria-label", label);
  const keys = activeKeys(hotkeys, "freeze");
  freezeBtn.title = on
    ? `${label}${keys ? `, ${keys}` : ""}`
    : `Freeze the screen: translate a still snapshot (${keys ? `${keys}; ` : ""}Esc goes back)`;
  lensEl.classList.toggle("frozen", on);
}

/** HK-02: name the current hotkeys in the tooltips. */
function setHotkeys(v: HotkeysView): void {
  hotkeys = v;
  copyBtn.title = `Copy translation${hint(v, "copy")}`;
  clickBtn.title = `Click-through interior${hint(v, "click_through")}`;
  const show = activeKeys(v, "show_hide");
  $("close").title = `Hide lens (${show ? `${show} or ` : ""}the tray brings it back)`;
  setLanguages(languages);
  setFrozen(frozen);
}

// MODE-01/02: switch between Overlay and Panel; Rust moves the window and answers `lens-strip`.
modeBtn.addEventListener("click", () => call("set_mode", { mode: modeButton(strip).next }));

clickBtn.addEventListener("click", () => {
  const on = clickBtn.getAttribute("aria-pressed") !== "true";
  setClickThroughUi(on);
  call("set_click_through", { enabled: on });
});

$("close").addEventListener("click", () => call("hide_lens"));
// REN-05: copy what the lens shows; Rust holds the text, so nothing is sent from here.
copyBtn.addEventListener("click", () => {
  if (!isTauri()) return;
  invoke<boolean>("copy_translation")
    .then((copied) => flashCopy(copied ? "✓" : "–", copied ? "Copied" : "Nothing to copy"))
    .catch((e: unknown) => {
      console.error("copy_translation failed", e);
      flashCopy("!", "Copying failed");
    });
});

// SV-01: swap the direction; both models stay loaded, so it is instant.
swapBtn.addEventListener("click", () => {
  if (!isTauri()) return;
  invoke<LanguagesView>("swap_languages")
    .then(setLanguages)
    .catch((e: unknown) => console.error("swap_languages failed", e));
});

// MDL-07: pick the pair. Rust checks it against the model catalog.
sourceEl.addEventListener("change", () => {
  const choice = parseSourceValue(sourceEl.value);
  if (choice) chooseLanguages(choice.source, languages.target, choice.auto);
});
targetEl.addEventListener("change", () => {
  const source = pickSource(languages, targetEl.value);
  if (source) chooseLanguages(source, targetEl.value, languages.auto);
});

function flashCopy(mark: string, label: string): void {
  copyBtn.textContent = mark;
  copyBtn.title = label;
  setTimeout(() => {
    copyBtn.textContent = "⧉";
    copyBtn.title = `Copy translation${hint(hotkeys, "copy")}`;
  }, 1200);
}
ocrLangBtn.addEventListener("click", () => call("open_ocr_language_settings"));
// MDL-01/03/05: the model manager does the downloading; ⬇ opens it (and Settings links to it).
modelsBtn.addEventListener("click", () => call("open_model_manager"));
settingsBtn.addEventListener("click", () => call("open_settings"));

async function start(): Promise<void> {
  if (!isTauri()) {
    // Plain browser (layout preview): show a sample update instead of live data.
    applyConfig({
      layout: { frame: 6, toolbar: 30, panel: 120 },
      mode: "demo",
      clickThrough: true,
      // Preview Panel mode with ?panel or ?above in the URL.
      strip: location.search.includes("above") ? "above" : location.search.includes("panel") ? "below" : "none",
      pipeline: true,
      pipelineError: null,
      modelsMissingBytes: 0,
      modelsDownloadable: true,
      installCommand: "python scripts/models/opus_mt.py",
      languages: {
        source: "sv",
        target: "en",
        auto: true,
        pairs: [
          { source: "en", target: "sv", installed: true },
          { source: "sv", target: "en", installed: true },
          { source: "de", target: "en", installed: false },
        ],
        canSwap: true,
      },
      traceTimings: false,
      // Preview the PRIV-02 banner with ?dump in the URL.
      dumpFrames: location.search.includes("dump") ? "C:\\frames" : null,
      // Preview LENS-07 with ?light (a light frame) or ?big (Georgia at 150 %, 60 % opacity).
      appearance: location.search.includes("light")
        ? { ...DEFAULT_APPEARANCE, frame_color: "#e8eef4" }
        : location.search.includes("big")
          ? { frame_color: "#3a1f5c", opacity: 60, font: "georgia", font_scale: 150 }
          : DEFAULT_APPEARANCE,
      hotkeys: DEFAULT_HOTKEYS,
    });
    render(sampleUpdate());
    return;
  }
  await listen<LensUpdate>("lens-update", (e) => render(e.payload));
  await listen<string>("lens-error", (e) => showMessage(message(e.payload)));
  await listen<ModelsView>("models-changed", (e) => onModelsView(e.payload));
  await listen<boolean>("lens-original", (e) => setShowOriginal(e.payload));
  await listen<LanguagesView>("lens-languages", (e) => setLanguages(e.payload));
  await listen<boolean>("lens-click-through", (e) => setClickThroughUi(e.payload));
  await listen<Strip>("lens-strip", (e) => setStrip(e.payload));
  await listen<boolean>("lens-frozen", (e) => setFrozen(e.payload));
  await listen<Appearance>("lens-appearance", (e) => setAppearance(e.payload));
  await listen<HotkeysView>("hotkeys-changed", (e) => setHotkeys(e.payload));
  applyConfig(await invoke<LensConfig>("lens_config"));
}

function sampleUpdate(): LensUpdate {
  const block = (y: number, source: string, translation: string, w = 260): RenderedBlock => ({
    rect: { x: 8, y, w, h: 18 },
    line_height: 18,
    source,
    source_lang: "sv",
    translation,
    target_lang: "en",
    passthrough: false,
    dim: false,
    bg: "#eceef2",
    fg: "#1a1a1a",
  });
  return {
    generation: 1,
    status: { kind: "ok" },
    blocks: [
      block(8, "Spara ändringar", "Save changes"),
      block(38, "Avbryt", "Cancel"),
      // Too long for its box even at 9 px: grows downward with the full text in its tooltip.
      block(68, "Ändringarna försvinner", "Your changes will be lost if you don't save them", 120),
    ],
    backend: { id: "demo", name: "demo", local: true },
    notices: [],
    timings: { capture: 0, change: 0, ocr: 0, group: 0, translate: 0, total: 0 },
  };
}

void start();
