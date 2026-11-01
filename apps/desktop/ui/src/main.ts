// Lens UI: a thin renderer for `LensUpdate` events plus the toolbar (PLAN §3.3). All state and
// validation live in Rust; this file only draws and forwards user input to commands.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MODEL_MESSAGES, formatBytes, percent } from "./catalog";
import { fitFontSize, maxHeight, startSize } from "./fit";
import type { LensConfig, LensUpdate, ModelsView, RenderedBlock } from "./types";

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
const blocksEl = $("blocks");
const messageEl = $("message");
const noticeEl = $("notice");
const privacyEl = $("privacy");
const privacyName = $("privacy-name");
const lockIcon = document.getElementById("icon-lock") as unknown as SVGElement;
const cloudIcon = document.getElementById("icon-cloud") as unknown as SVGElement;
const pauseBtn = $<HTMLButtonElement>("pause");
const copyBtn = $<HTMLButtonElement>("copy");
const swapBtn = $<HTMLButtonElement>("swap");
const targetEl = $("target-lang");
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
/** Progress text while the model manager downloads. */
let downloadProgress: string | null = null;
/** Whether the missing models can be downloaded (false: import-only, see README). */
let modelsDownloadable = true;
/** Set when a download found the models release unreachable (e.g. a private repository). */
let modelsUnreachable = false;
/** The last model download or delete outcome already shown. */
let modelsMessage: string | null = null;
/** `--trace-timings`: tell Rust when each update has been painted. */
let traceTimings = false;

// Fluent IDs from the pipeline; real localized strings arrive with M4-5.
const MESSAGES: Record<string, string> = {
  "no-text": "No text found",
  "cap-error": "Screen capture failed",
  "cap-error-permission": "Screen capture permission is missing",
  "ocr-error": "Text recognition failed",
  "ocr-error-language-missing": "Text recognition language is not installed",
  "cap-error-self-exclusion":
    "The lens can't hide itself from screen capture (needs Windows 10 version 2004 or later), so live capture is off.",
  "tr-error-model-missing":
    "The Swedish ↔ English translation models aren't downloaded yet. Click ⬇ in the toolbar to download them (about 170 MB, once).",
  // For import-only manifest entries (no download URLs): installed from the repository.
  "tr-error-model-import":
    "The Swedish ↔ English translation models aren't installed yet. Install them once from a terminal in the Polyglass repository with: python scripts/models/opus_mt.py (about 170 MB; README → First run), then restart Polyglass.",
  "tr-error-backend": "Translation failed",
  ...MODEL_MESSAGES,
  // The lens has room for the whole fallback, so it carries the command too.
  "mdl-error-unreachable":
    "The translation models can't be downloaded: their release isn't reachable (the Polyglass repository may be private). Install them once from a terminal in the Polyglass repository with: python scripts/models/opus_mt.py (about 170 MB; README → First run), then restart Polyglass.",
};

// SV-02: where to add Swedish OCR on Windows. The toolbar's ⚠ SV button opens the page.
const SWEDISH_OCR_HELP =
  "Swedish text recognition isn't installed, so only English is read. Add it in Windows Settings → Time & language → Language & region → Swedish → Language options → Optical character recognition (⚠ SV in the toolbar opens Settings).";

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
  if (HEX.test(b.bg)) el.style.backgroundColor = b.bg;
  if (HEX.test(b.fg)) el.style.color = b.fg;
  return el;
}

/**
 * REN-02: start at the source text's size and shrink to fit the source box, down to 9 px. Text
 * that still doesn't fit grows the box downward to the interior's bottom edge, and the block's
 * tooltip holds the full text (seen with click-through off; Panel view is M2-2).
 */
function fit(el: HTMLElement, b: RenderedBlock): void {
  const { rect } = b;
  const { size, overflow } = fitFontSize((px) => {
    el.style.fontSize = `${px}px`;
    return el.scrollHeight <= rect.h + 1 && el.scrollWidth <= rect.w + 1;
  }, startSize(b.line_height));
  el.style.fontSize = `${size}px`;
  el.classList.toggle("overflow", overflow);
  if (overflow) {
    el.style.maxHeight = `${maxHeight(rect.y, blocksEl.clientHeight)}px`;
    el.title = el.textContent ?? "";
  }
}

function drawBlocks(u: LensUpdate): void {
  const els = u.blocks.map(renderBlock);
  blocksEl.replaceChildren(...els);
  els.forEach((el, i) => fit(el, u.blocks[i]!));
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
      showMessage(
        id === "tr-error-model-missing" && downloadProgress ? downloadProgress : (MESSAGES[id] ?? id),
      );
      break;
    }
  }
  // Live mode only falls back to the mock if someone wires it; say so when it happens.
  if (mode === "live") setBadge(u.backend.id === "mock" ? "mock" : null);

  const missing =
    u.notices.some((n) => n.kind === "ocr_language_missing") ||
    (u.status.kind === "error" && u.status.message_id === "ocr-error-language-missing");
  noticeEl.hidden = !missing;
  noticeEl.textContent = missing ? SWEDISH_OCR_HELP : "";
  ocrLangBtn.hidden = !missing;
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

function applyConfig(cfg: LensConfig): void {
  const root = document.documentElement.style;
  root.setProperty("--frame", `${cfg.layout.frame}px`);
  root.setProperty("--toolbar", `${cfg.layout.toolbar}px`);
  mode = cfg.mode;
  setBadge(cfg.mode === "demo" ? "demo" : null);
  modelsDownloadable = cfg.modelsDownloadable;
  setModelsButton(cfg.modelsMissingBytes);
  setClickThroughUi(cfg.clickThrough);
  setTarget(cfg.target);
  setDebugFlags(cfg.traceTimings, cfg.dumpFrames);
  if (cfg.pipelineError) {
    showMessage(MESSAGES[cfg.pipelineError] ?? cfg.pipelineError);
  } else if (!cfg.pipeline) {
    showMessage("Live capture isn't available on this platform yet. Start with --demo for mock results.");
  }
}

const LANG_NAMES: Record<string, string> = { en: "English", sv: "Swedish" };

function setTarget(target: string): void {
  targetEl.textContent = target.toUpperCase();
  const label = `Target language: ${LANG_NAMES[target] ?? target}`;
  targetEl.setAttribute("aria-label", label);
  targetEl.title = label;
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

/** ⬇ opens the model manager while the Swedish models are missing and can be downloaded. */
function setModelsButton(missingBytes: number): void {
  modelsBtn.hidden = !(mode === "live" && missingBytes > 0 && modelsDownloadable);
  const label = `Download the Swedish and English translation models (${formatBytes(missingBytes)})`;
  modelsBtn.title = label;
  modelsBtn.setAttribute("aria-label", label);
}

/** The model manager's state changed: mirror its download progress and outcome in the lens. */
function onModelsView(v: ModelsView): void {
  modelsDownloadable = v.swedish_downloadable;
  modelsUnreachable = v.unreachable;
  if (v.job) {
    const pct = percent(v.job);
    modelsBtn.textContent = `⬇ ${pct}%`;
    downloadProgress = `Downloading the translation models… ${pct}% of ${formatBytes(v.job.total)}`;
    showMessage(downloadProgress);
  } else {
    modelsBtn.textContent = "⬇";
    if (downloadProgress) {
      // The download ended: say how, or clear the progress for the next update.
      showMessage(v.message_id ? (MESSAGES[v.message_id] ?? v.message_id) : null);
    }
    downloadProgress = null;
  }
  if (v.message_id !== modelsMessage && v.message_id === "mdl-error-unreachable") {
    showMessage(MESSAGES[v.message_id] ?? v.message_id);
  }
  modelsMessage = v.message_id;
  setModelsButton(v.swedish_missing_bytes);
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
  const d = ARROWS[e.key];
  if (!d || e.altKey || e.ctrlKey || e.metaKey) return;
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
    current = null;
  }
  call("set_paused", { paused });
});

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
  invoke<string>("swap_languages")
    .then(setTarget)
    .catch((e: unknown) => console.error("swap_languages failed", e));
});

function flashCopy(mark: string, label: string): void {
  copyBtn.textContent = mark;
  copyBtn.title = label;
  setTimeout(() => {
    copyBtn.textContent = "⧉";
    copyBtn.title = "Copy translation";
  }, 1200);
}
ocrLangBtn.addEventListener("click", () => call("open_ocr_language_settings"));
// MDL-01/03/05: the model manager does the downloading; ⬇ and ⚙ open it.
modelsBtn.addEventListener("click", () => call("open_model_manager"));
settingsBtn.addEventListener("click", () => call("open_model_manager"));

async function start(): Promise<void> {
  if (!isTauri()) {
    // Plain browser (layout preview): show a sample update instead of live data.
    applyConfig({
      layout: { frame: 6, toolbar: 30 },
      mode: "demo",
      clickThrough: true,
      pipeline: true,
      pipelineError: null,
      modelsMissingBytes: 0,
      modelsDownloadable: true,
      target: "en",
      traceTimings: false,
      // Preview the PRIV-02 banner with ?dump in the URL.
      dumpFrames: location.search.includes("dump") ? "C:\\frames" : null,
    });
    render(sampleUpdate());
    return;
  }
  await listen<LensUpdate>("lens-update", (e) => render(e.payload));
  await listen<string>("lens-error", (e) => showMessage(MESSAGES[e.payload] ?? e.payload));
  await listen<ModelsView>("models-changed", (e) => onModelsView(e.payload));
  await listen<boolean>("lens-original", (e) => setShowOriginal(e.payload));
  await listen<string>("lens-target", (e) => setTarget(e.payload));
  await listen<boolean>("lens-click-through", (e) => setClickThroughUi(e.payload));
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
