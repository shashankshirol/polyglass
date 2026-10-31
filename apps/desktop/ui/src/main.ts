// Lens UI: a thin renderer for `LensUpdate` events plus the toolbar (PLAN §3.3). All state and
// validation live in Rust; this file only draws and forwards user input to commands.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LensConfig, LensUpdate, ModelEvent, RenderedBlock } from "./types";

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
const clickBtn = $<HTMLButtonElement>("click-through");
const ocrLangBtn = $<HTMLButtonElement>("ocr-lang");
const modelsBtn = $<HTMLButtonElement>("models");
const badge = $("mode-badge");

const HEX = /^#[0-9a-fA-F]{6}$/;
let lastGeneration = -1;
let paused = false;
let mode: LensConfig["mode"] = "none";
/** Progress text while the toolbar's model download runs. */
let downloadProgress: string | null = null;
/** Whether the missing models can be downloaded with ⬇ (false: import-only, see README). */
let modelsDownloadable = true;

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
    "The Swedish ↔ English translation models aren't downloaded yet. Click ⬇ in the toolbar to get them (about 170 MB, once).",
  // Until the converted OPUS-MT models are hosted (D-9) they're installed from the repository.
  "tr-error-model-import":
    "The Swedish ↔ English translation models aren't installed yet. Install them once from a terminal in the Polyglass repository with: python scripts/models/opus_mt.py (about 170 MB; README → First run), then restart Polyglass.",
  "tr-error-backend": "Translation failed",
  "mdl-error-download": "Downloading the translation models failed. Check the connection and click ⬇ to try again.",
};

const MB = (bytes: number): string => `${(bytes / 1e6).toFixed(1)} MB`;

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
  el.textContent = b.translation ?? b.source;
  el.lang = b.translation && !b.passthrough ? b.target_lang : b.source_lang;
  el.style.left = `${b.rect.x}px`;
  el.style.top = `${b.rect.y}px`;
  el.style.width = `${b.rect.w}px`;
  el.style.minHeight = `${b.rect.h}px`;
  if (HEX.test(b.bg)) el.style.backgroundColor = b.bg;
  if (HEX.test(b.fg)) el.style.color = b.fg;
  return el;
}

/** Basic fit until REN-02 (M1-APP-6): shrink from 16 px towards 9 px until the block fits. */
function fit(el: HTMLElement, rect: RenderedBlock["rect"]): void {
  for (let size = 16; size >= 9; size--) {
    el.style.fontSize = `${size}px`;
    if (el.scrollHeight <= rect.h + 1 && el.scrollWidth <= rect.w + 1) return;
  }
}

function render(u: LensUpdate): void {
  // The driver never emits stale generations (CAP-03); this guards against reordering anyway.
  if (u.generation < lastGeneration) return;
  lastGeneration = u.generation;

  setPrivacy(u.backend.local, u.backend.name);
  const els = u.blocks.map(renderBlock);
  blocksEl.replaceChildren(...els);
  els.forEach((el, i) => fit(el, u.blocks[i]!.rect));

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
          ? "tr-error-model-import"
          : u.status.message_id;
      if (id === "tr-error-model-missing") modelsBtn.hidden = false;
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

function applyConfig(cfg: LensConfig): void {
  const root = document.documentElement.style;
  root.setProperty("--frame", `${cfg.layout.frame}px`);
  root.setProperty("--toolbar", `${cfg.layout.toolbar}px`);
  mode = cfg.mode;
  setBadge(cfg.mode === "demo" ? "demo" : null);
  modelsDownloadable = cfg.modelsDownloadable;
  if (cfg.mode === "live" && cfg.modelsMissingBytes > 0 && cfg.modelsDownloadable) {
    modelsBtn.hidden = false;
    const label = `Download the Swedish and English translation models (${MB(cfg.modelsMissingBytes)})`;
    modelsBtn.title = label;
    modelsBtn.setAttribute("aria-label", label);
  }
  setClickThroughUi(cfg.clickThrough);
  if (cfg.pipelineError) {
    showMessage(MESSAGES[cfg.pipelineError] ?? cfg.pipelineError);
  } else if (!cfg.pipeline) {
    showMessage("Live capture isn't available on this platform yet. Start with --demo for mock results.");
  }
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

function onModelEvent(e: ModelEvent): void {
  switch (e.state) {
    case "downloading": {
      const pct = Math.floor((e.downloaded * 100) / Math.max(e.total, 1));
      modelsBtn.disabled = true;
      modelsBtn.textContent = `⬇ ${pct}%`;
      downloadProgress = `Downloading the translation models… ${pct}% of ${MB(e.total)}`;
      showMessage(downloadProgress);
      break;
    }
    case "ready":
      downloadProgress = null;
      modelsBtn.hidden = true;
      showMessage(null);
      break;
    case "error":
      downloadProgress = null;
      modelsBtn.disabled = false;
      modelsBtn.textContent = "⬇";
      showMessage(MESSAGES[e.message_id] ?? e.message_id);
      break;
  }
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
  if (paused) blocksEl.replaceChildren();
  call("set_paused", { paused });
});

clickBtn.addEventListener("click", () => {
  const on = clickBtn.getAttribute("aria-pressed") !== "true";
  setClickThroughUi(on);
  call("set_click_through", { enabled: on });
});

$("close").addEventListener("click", () => call("hide_lens"));
ocrLangBtn.addEventListener("click", () => call("open_ocr_language_settings"));
modelsBtn.addEventListener("click", () => {
  modelsBtn.disabled = true;
  call("download_models");
});

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
    });
    render(sampleUpdate());
    return;
  }
  await listen<LensUpdate>("lens-update", (e) => render(e.payload));
  await listen<string>("lens-error", (e) => showMessage(MESSAGES[e.payload] ?? e.payload));
  await listen<ModelEvent>("lens-models", (e) => onModelEvent(e.payload));
  applyConfig(await invoke<LensConfig>("lens_config"));
}

function sampleUpdate(): LensUpdate {
  const block = (y: number, source: string, translation: string): RenderedBlock => ({
    rect: { x: 8, y, w: 260, h: 18 },
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
    blocks: [block(8, "Spara ändringar", "Save changes"), block(38, "Avbryt", "Cancel")],
    backend: { id: "demo", name: "demo", local: true },
    notices: [],
    timings: { capture: 0, change: 0, ocr: 0, group: 0, translate: 0, total: 0 },
  };
}

void start();
