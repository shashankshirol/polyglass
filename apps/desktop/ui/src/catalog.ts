// What the model manager and the wizard show for a `ModelsView` (MDL-01/03/05, SV-01). Pure
// functions, tested with `npm test`; Rust decides what is allowed, this only picks what to draw.

import { langName } from "./langs.ts";
import type { JobView, ModelRow, ModelsView } from "./types";

// Message IDs from `models.rs` and their text. The UI is in English in v1 (D-17).
export const MODEL_MESSAGES: Record<string, string> = {
  "mdl-cancelled": "Download stopped. It continues where it stopped when you download again.",
  "mdl-error-download": "Downloading the translation models failed. Check the connection and try again.",
  "mdl-error-verify":
    "A downloaded file didn't match its checksum, so it was deleted. Try again; if it keeps happening, the download source may have changed.",
  "mdl-error-disk": "The models couldn't be saved. Check that there is free disk space.",
  "mdl-error-delete": "The model couldn't be deleted. Close other programs using its files and try again.",
  "mdl-error-unreachable":
    "The translation models can't be downloaded: their release isn't reachable (the Polyglass repository may be private).",
  "mdl-import-none":
    "No model files from the catalog in that folder. It needs the models release's files (named like opus-mt-sv-en.model.bin), or the folders the install script writes.",
};

/** `models::ImportOutcome`. */
export interface ImportOutcome {
  imported: string[];
  failed: string[];
  message_id: string | null;
}

/** What "Import from a folder" says afterwards (MDL-04), naming the pairs by their rows. */
export function importText(out: ImportOutcome, rows: ModelRow[]): string {
  const names = (ids: string[]): string =>
    ids.map((id) => rows.find((r) => r.id === id)).map((r, i) => (r ? pairName(r) : ids[i])).join(" and ");
  const parts: string[] = [];
  if (out.imported.length > 0) {
    parts.push(`Imported ${names(out.imported)}, checked against the catalog and ready to use.`);
  }
  if (out.failed.length > 0) {
    const why =
      out.message_id === "mdl-error-verify"
        ? "a file didn't match its checksum, so it was not kept"
        : "its files couldn't be copied; check that there is free disk space";
    parts.push(`${names(out.failed)} wasn't imported: ${why}.`);
  }
  if (parts.length === 0) parts.push(MODEL_MESSAGES[out.message_id ?? "mdl-import-none"] ?? "");
  return parts.join(" ");
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

export function pairName(row: Pick<ModelRow, "source" | "target">): string {
  return `${langName(row.source)} → ${langName(row.target)}`;
}

export function percent(job: JobView): number {
  return Math.min(100, Math.floor((job.downloaded * 100) / Math.max(job.total, 1)));
}

export type RowState = "installed" | "downloading" | "partial" | "missing";

export function rowState(row: ModelRow, view: Pick<ModelsView, "job">): RowState {
  if (view.job?.ids.includes(row.id)) return "downloading";
  if (row.installed) return "installed";
  return row.on_disk > 0 ? "partial" : "missing";
}

/** Whether a missing model can be fetched in this run (URLs in the manifest, release reachable). */
export function canDownload(row: ModelRow, view: Pick<ModelsView, "unreachable">): boolean {
  return row.downloadable && !view.unreachable;
}

export interface RowActions {
  /** "Download" or "Resume", or none. */
  download: "Download" | "Resume" | null;
  /** "Delete" for an installed model, "Discard" for a partial download, or none. */
  remove: "Delete" | "Discard" | null;
  /** Show the install command instead of a download button (import-only or unreachable). */
  installCommand: boolean;
}

export function rowActions(row: ModelRow, view: Pick<ModelsView, "job" | "unreachable">): RowActions {
  const state = rowState(row, view);
  if (state === "downloading") return { download: null, remove: null, installCommand: false };
  if (state === "installed") return { download: null, remove: "Delete", installCommand: false };
  const remove = state === "partial" ? "Discard" : null;
  if (!canDownload(row, view)) return { download: null, remove, installCommand: true };
  return { download: state === "partial" ? "Resume" : "Download", remove, installCommand: false };
}

/** Bytes the selected, not yet installed models download (SV-01: shown before downloading). */
export function selectedBytes(rows: ModelRow[], selected: ReadonlySet<string>): number {
  return rows
    .filter((r) => selected.has(r.id) && !r.installed)
    .reduce((sum, r) => sum + r.download_size, 0);
}

export type WizardStage = "offer" | "downloading" | "ready" | "install-command";

/** Which step of the first-run wizard to show. */
export function wizardStage(view: ModelsView): WizardStage {
  if (view.job) return "downloading";
  if (view.wanted_missing_bytes === 0) return "ready";
  if (!view.wanted_downloadable) return "install-command";
  return "offer";
}
