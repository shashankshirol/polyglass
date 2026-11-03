// Model manager and first-run wizard window (MDL-01/03/05/06, SV-01). Rust owns the catalog,
// the downloads and every check; this page draws `ModelsView` and forwards clicks to commands.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  MODEL_MESSAGES,
  formatBytes,
  importText,
  pairName,
  percent,
  rowActions,
  rowState,
  selectedBytes,
  wizardStage,
  type ImportOutcome,
} from "./catalog";
import { langName } from "./langs";
import type { ModelRow, ModelsView } from "./types";

const $ = <T extends HTMLElement>(id: string): T => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing #${id}`);
  return el as T;
};

const app = $("app");
const jobEl = $("job");
const jobLabel = $("job-label");
const jobProgress = $<HTMLProgressElement>("job-progress");
const cancelBtn = $<HTMLButtonElement>("cancel");
const messageEl = $("message");
const installEl = $("install");
const installCommand = $("install-command");
const wizardChoices = $("wizard-choices");
const wizardTotal = $("wizard-total");
const wizardDownload = $<HTMLButtonElement>("wizard-download");
const rowsEl = $("rows");
const wizardIntro = $("wizard-intro");
const wizardReady = $("wizard-ready-text");
const usageEl = $("usage");
const importPath = $<HTMLInputElement>("import-path");
const importGo = $<HTMLButtonElement>("import-go");
const importMessage = $("import-message");

let view: ModelsView | null = null;
/** Wizard checkboxes; starts with the suggested models (SV-01) and keeps the user's choice. */
let selected: Set<string> | null = null;
/** The delete button waiting for a second click, and when it stops waiting. */
let confirming: { id: string; until: number } | null = null;

const STATE_TEXT = {
  installed: "Installed",
  downloading: "Downloading",
  partial: "Partly downloaded",
  missing: "Not installed",
} as const;

function show(id: string, on: boolean): void {
  $(id).hidden = !on;
}

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  if (!isTauri()) return Promise.resolve(undefined);
  return invoke<T>(cmd, args).catch((e: unknown) => {
    console.error(`${cmd} failed`, e);
    return undefined;
  });
}

function render(v: ModelsView): void {
  view = v;
  app.removeAttribute("aria-busy");
  const wizard = v.first_run;
  show("wizard-head", wizard);
  show("wizard", wizard);
  show("manager-head", !wizard);
  show("manager", !wizard);
  document.title = wizard ? "Welcome to Polyglass" : "Polyglass – Translation models";

  renderJob(v);
  messageEl.hidden = v.message_id === null;
  messageEl.textContent = v.message_id ? (MODEL_MESSAGES[v.message_id] ?? v.message_id) : "";
  messageEl.classList.toggle("error", v.message_id?.startsWith("mdl-error") ?? false);
  installCommand.textContent = v.install_command;

  if (wizard) renderWizard(v);
  else renderManager(v);
  // MDL-04: in the manager, and in the wizard when the models can't be downloaded.
  show("import", !wizard || wizardStage(v) === "install-command");
  importGo.disabled = v.job !== null;
}

function renderJob(v: ModelsView): void {
  jobEl.hidden = v.job === null;
  if (!v.job) return;
  const names = v.rows.filter((r) => v.job?.ids.includes(r.id)).map(pairName);
  const pct = percent(v.job);
  jobLabel.textContent = `Downloading ${names.join(" and ")}: ${pct}% of ${formatBytes(v.job.total)}`;
  jobProgress.value = pct;
  jobProgress.setAttribute("aria-valuetext", `${pct}%`);
}

function renderWizard(v: ModelsView): void {
  const stage = wizardStage(v);
  // The selected direction first (Swedish → English by default, SV-01).
  const offered = v.wanted_ids.flatMap((id) => v.rows.filter((r) => r.id === id));
  const pairText = offered[0] ? pairLabel(offered) : "";
  wizardIntro.textContent = `To translate ${pairText} it needs ${offered.length === 1 ? "a translation model" : `${offered.length} translation models`}, downloaded once from the Polyglass releases on GitHub. Nothing is downloaded until you choose Download.`;
  wizardReady.textContent = `${pairText.replace(/^between /, "")} is installed. Point the lens at the text; the toolbar picks the languages${offered.length > 1 ? ", and ⇄ or Ctrl+Alt+S swaps the direction" : ""}.`;
  show("wizard-offer", stage === "offer");
  show("wizard-ready", stage === "ready");
  show("wizard-close", stage === "ready" || stage === "install-command");
  installEl.hidden = stage !== "install-command";

  selected ??= new Set(offered.filter((r) => !r.installed).map((r) => r.id));
  const boxes = offered.map((r) => choice(r, selected!));
  wizardChoices.replaceChildren(...boxes);
  updateWizardTotal(v);
}

/** "between Swedish and English" for a pair and its reverse, else "German into English". */
function pairLabel(rows: ModelRow[]): string {
  const first = rows[0]!;
  const back = rows.some((r) => r.source === first.target && r.target === first.source);
  return back
    ? `between ${langName(first.source)} and ${langName(first.target)}`
    : `${langName(first.source)} into ${langName(first.target)}`;
}

function choice(r: ModelRow, sel: Set<string>): HTMLElement {
  const label = document.createElement("label");
  label.className = "choice";
  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = r.installed || sel.has(r.id);
  box.disabled = r.installed;
  box.addEventListener("change", () => {
    if (box.checked) sel.add(r.id);
    else sel.delete(r.id);
    if (view) updateWizardTotal(view);
  });
  const name = document.createElement("span");
  name.className = "choice-name";
  name.textContent = pairName(r);
  const meta = document.createElement("span");
  meta.className = "choice-meta";
  meta.textContent = r.installed
    ? "installed"
    : `${formatBytes(r.download_size)} · ${r.license}${r.on_disk > 0 ? " · resumes" : ""}`;
  label.append(box, name, meta);
  return label;
}

function updateWizardTotal(v: ModelsView): void {
  const sel = selected ?? new Set<string>();
  const bytes = selectedBytes(v.rows, sel);
  const licenses = [...new Set(v.rows.filter((r) => sel.has(r.id)).map((r) => r.license))];
  wizardTotal.textContent =
    bytes > 0
      ? `Download size: ${formatBytes(bytes)} together, once. License: ${licenses.join(", ")} (credits in the model manager).`
      : "Nothing selected.";
  wizardDownload.textContent = bytes > 0 ? `Download ${formatBytes(bytes)}` : "Download";
  wizardDownload.disabled = bytes === 0;
}

function renderManager(v: ModelsView): void {
  const needsCommand = v.rows.some((r) => rowActions(r, v).installCommand && !r.installed);
  installEl.hidden = !needsCommand;
  rowsEl.replaceChildren(...v.rows.map((r) => renderRow(r, v)));
  usageEl.textContent = `Models use ${formatBytes(v.disk_usage)} in ${v.store_path}`;
}

function renderRow(r: ModelRow, v: ModelsView): HTMLElement {
  const state = rowState(r, v);
  const actions = rowActions(r, v);
  const li = document.createElement("li");
  li.className = "row";
  li.dataset.state = state;
  const name = pairName(r);

  const head = document.createElement("div");
  head.className = "row-head";
  const h = document.createElement("h2");
  h.textContent = name;
  const badge = document.createElement("span");
  badge.className = "state";
  badge.textContent =
    state === "partial"
      ? `${STATE_TEXT.partial} (${formatBytes(r.on_disk)} of ${formatBytes(r.download_size)})`
      : STATE_TEXT[state];
  head.append(h, badge);

  const meta = document.createElement("dl");
  meta.className = "meta";
  const add = (term: string, value: string, cls?: string): void => {
    const dt = document.createElement("dt");
    dt.textContent = term;
    const dd = document.createElement("dd");
    dd.textContent = value;
    if (cls) dd.className = cls;
    meta.append(dt, dd);
  };
  add("Download size", formatBytes(r.download_size));
  add("License", r.license);
  add("Source", r.source_url, "url");
  add("Version", r.version, "mono");
  if (state === "installed") add("On disk", formatBytes(r.on_disk));

  li.append(head, meta);
  if (r.attribution) {
    const details = document.createElement("details");
    const summary = document.createElement("summary");
    summary.textContent = "Attribution";
    const p = document.createElement("p");
    p.textContent = r.attribution;
    details.append(summary, p);
    li.append(details);
  }

  const bar = document.createElement("div");
  bar.className = "actions";
  if (actions.download) {
    const b = button(`${actions.download} ${formatBytes(r.download_size - r.on_disk)}`, `${actions.download} ${name}`);
    b.classList.add("primary");
    b.disabled = v.job !== null;
    b.addEventListener("click", () => void call("download_models", { ids: [r.id] }));
    bar.append(b);
  }
  if (actions.installCommand) {
    const note = document.createElement("span");
    note.className = "note";
    note.textContent = r.downloadable
      ? "Download unavailable: install with the command above."
      : "Not hosted yet: install with the command above.";
    bar.append(note);
  }
  if (actions.remove) {
    const waiting = confirming?.id === r.id && confirming.until > Date.now();
    const label = waiting ? `Really ${actions.remove.toLowerCase()}?` : actions.remove;
    const b = button(label, `${waiting ? "Confirm: " : ""}${actions.remove} ${name}`);
    b.classList.add("danger");
    b.addEventListener("click", () => onRemove(r.id));
    bar.append(b);
  }
  if (bar.childElementCount > 0) li.append(bar);
  return li;
}

function button(text: string, label: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = text;
  b.setAttribute("aria-label", label);
  return b;
}

/** MDL-05: delete takes a second click within 4 s. */
function onRemove(id: string): void {
  if (confirming?.id === id && confirming.until > Date.now()) {
    confirming = null;
    void call("delete_model", { id });
    return;
  }
  confirming = { id, until: Date.now() + 4000 };
  if (view) render(view);
  setTimeout(() => {
    if (confirming?.id === id && confirming.until <= Date.now()) {
      confirming = null;
      if (view) render(view);
    }
  }, 4100);
  // Keep keyboard focus on the button that now asks for confirmation.
  rowsEl.querySelector<HTMLButtonElement>(`[aria-label^="Confirm: "]`)?.focus();
}

/** MDL-04: import what a folder holds; Rust checks the path and every file. */
async function importFrom(path: string): Promise<void> {
  if (!path.trim()) {
    importPath.focus();
    return;
  }
  if (!isTauri()) return;
  importGo.disabled = true;
  importMessage.hidden = false;
  importMessage.classList.remove("error");
  importMessage.textContent = "Importing and checking the files…";
  const out = await invoke<ImportOutcome>("import_models", { path }).catch((e: unknown) => {
    importMessage.textContent = `Can't import from there: ${String(e)}.`;
    importMessage.classList.add("error");
    return null;
  });
  importGo.disabled = view?.job != null;
  if (!out) return;
  importMessage.textContent = importText(out, view?.rows ?? []);
  importMessage.classList.toggle("error", out.imported.length === 0 || out.failed.length > 0);
}

importGo.addEventListener("click", () => void importFrom(importPath.value));
importPath.addEventListener("keydown", (e) => {
  if (e.key === "Enter") void importFrom(importPath.value);
});

cancelBtn.addEventListener("click", () => void call("cancel_download"));
wizardDownload.addEventListener("click", () => {
  const ids = [...(selected ?? [])];
  if (ids.length > 0) void call("download_models", { ids });
});
$("wizard-skip").addEventListener("click", () => void call("finish_first_run"));
$("wizard-done").addEventListener("click", () => void call("finish_first_run"));

async function start(): Promise<void> {
  if (!isTauri()) {
    // Plain browser (layout preview): a sample catalog.
    render(sampleView(new URLSearchParams(location.search).has("wizard")));
    return;
  }
  await listen<ModelsView>("models-changed", (e) => render(e.payload));
  // MDL-04: a folder (or a file in it) dropped on the window is imported.
  await getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type === "drop" && e.payload.paths[0]) {
      importPath.value = e.payload.paths[0];
      void importFrom(importPath.value);
    }
  });
  // Redraw from disk when the window comes back to the front (the install script may have run).
  window.addEventListener("focus", () => void refresh());
  await refresh();
}

async function refresh(): Promise<void> {
  const v = await call<ModelsView>("models_state");
  if (v) render(v);
}

function sampleView(wizard: boolean): ModelsView {
  const row = (id: string, source: string, target: string, extra: Partial<ModelRow>): ModelRow => ({
    id,
    source,
    target,
    version: "opus+bt-2021-04-30-ct2-int8",
    download_size: 84_831_168,
    downloadable: true,
    license: "CC-BY-4.0",
    attribution:
      "OPUS-MT model by Helsinki-NLP (University of Helsinki), CC-BY 4.0; converted to CTranslate2 int8 by Polyglass.",
    source_url: "https://github.com/Helsinki-NLP/Tatoeba-Challenge",
    installed: false,
    on_disk: 0,
    suggested: true,
    ...extra,
  });
  return {
    rows: [
      row("opus-mt-en-sv", "en", "sv", { on_disk: 31_000_000 }),
      row("opus-mt-sv-en", "sv", "en", { installed: !wizard, on_disk: wizard ? 0 : 84_831_200 }),
    ],
    disk_usage: wizard ? 0 : 115_831_200,
    store_path: "C:\\Users\\you\\AppData\\Local\\org.polyglass.desktop\\models",
    job: null,
    message_id: wizard ? null : "mdl-cancelled",
    unreachable: false,
    first_run: wizard,
    install_command: "python scripts/models/opus_mt.py",
    wanted_ids: ["opus-mt-sv-en", "opus-mt-en-sv"],
    wanted_missing_bytes: 169_662_336,
    wanted_downloadable: true,
  };
}

void start();
