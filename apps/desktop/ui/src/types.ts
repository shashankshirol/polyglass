// Mirrors `pg_pipeline::update` (serde output). The UI only draws these (PLAN §3.3).

import type { Appearance } from "./appearance";
import type { HotkeysView } from "./hotkeys";

export interface LensUpdate {
  generation: number;
  status: LensStatus;
  blocks: RenderedBlock[];
  backend: BackendInfo;
  notices: Notice[];
  timings: Timings;
}

export type LensStatus =
  | { kind: "ok" }
  | { kind: "no_text_found" }
  | { kind: "error"; message_id: string; lang?: string };

export type Notice = { kind: "ocr_language_missing"; lang: string };

export interface BackendInfo {
  id: string;
  name: string;
  local: boolean;
}

export interface LensRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface RenderedBlock {
  rect: LensRect;
  /** Mean height of the source lines, lens-local logical px (REN-02 starting size). */
  line_height: number;
  source: string;
  source_lang: string;
  translation: string | null;
  target_lang: string;
  passthrough: boolean;
  dim: boolean;
  bg: string;
  fg: string;
}

export interface Timings {
  capture: number;
  change: number;
  ocr: number;
  group: number;
  translate: number;
  total: number;
}

/** `commands::LensConfig`. */
export interface LensConfig {
  layout: { frame: number; toolbar: number; panel: number };
  /** demo: all mocks; live: real capture, OCR and on-device translation. */
  mode: "demo" | "live" | "none";
  clickThrough: boolean;
  /** Where the Panel mode strip is; "none" in Overlay mode (MODE-02). */
  strip: "none" | "below" | "above";
  pipeline: boolean;
  /** Fluent message ID if the pipeline failed to start. */
  pipelineError: string | null;
  /** Bytes of the selected pair's models still to download (0 = installed). */
  modelsMissingBytes: number;
  /** False if a missing model is import-only (no download URLs in the manifest). */
  modelsDownloadable: boolean;
  /** How to install the models when they can't be downloaded (MDL-04). */
  installCommand: string;
  /** The language pair and what the pickers offer (MDL-07). */
  languages: LanguagesView;
  /** `--trace-timings`: report each drawn update with `lens_drawn`. */
  traceTimings: boolean;
  /** `--debug-dump-frames`: the folder frames are saved to (PRIV-02 red banner). */
  dumpFrames: string | null;
  /** LENS-07; `lens-appearance` events carry later changes. */
  appearance: Appearance;
  /** HK-02, for the tooltips; `hotkeys-changed` events carry later changes. */
  hotkeys: HotkeysView;
}

/** `langs::LanguagesView`: the pair in use and every pair the model catalog lists. */
export interface LanguagesView {
  source: string;
  target: string;
  /** Each block is identified as `source` or `target` (SV-04). */
  auto: boolean;
  pairs: { source: string; target: string; installed: boolean }[];
  /** The catalog has the reverse pair (⇄). */
  canSwap: boolean;
}

/** `models::ModelsView`: the model manager's state, sent with `models-changed` events. */
export interface ModelsView {
  /** MDL-01 catalog, Swedish pairs first. */
  rows: ModelRow[];
  /** Bytes the model store uses, partial downloads included (MDL-05). */
  disk_usage: number;
  store_path: string;
  job: JobView | null;
  /** Fluent ID of the last download or delete outcome worth telling. */
  message_id: string | null;
  /** The release isn't reachable; downloads are off for this run. */
  unreachable: boolean;
  /** The window is the first-run wizard (SV-01). */
  first_run: boolean;
  install_command: string;
  /** The selected pair's models, the selected direction first. */
  wanted_ids: string[];
  /** Bytes of the selected pair's models still missing (0 = installed). */
  wanted_missing_bytes: number;
  wanted_downloadable: boolean;
}

export interface ModelRow {
  id: string;
  source: string;
  target: string;
  version: string;
  download_size: number;
  /** False for import-only entries (no download URLs). */
  downloadable: boolean;
  license: string;
  attribution: string | null;
  source_url: string;
  installed: boolean;
  /** Bytes on disk: the files when installed, else what a download resumes from. */
  on_disk: number;
  /** A model of the selected pair; pre-selected by the first-run wizard. */
  suggested: boolean;
}

export interface JobView {
  ids: string[];
  downloaded: number;
  total: number;
}
