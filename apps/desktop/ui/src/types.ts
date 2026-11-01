// Mirrors `pg_pipeline::update` (serde output). The UI only draws these (PLAN §3.3).

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
  layout: { frame: number; toolbar: number };
  /** demo: all mocks; live: real capture, OCR and on-device translation. */
  mode: "demo" | "live" | "none";
  clickThrough: boolean;
  pipeline: boolean;
  /** Fluent message ID if the pipeline failed to start. */
  pipelineError: string | null;
  /** Bytes of Swedish ↔ English models still to download (0 = installed). */
  modelsMissingBytes: number;
  /** False if a missing model is import-only (no download URLs in the manifest). */
  modelsDownloadable: boolean;
  /** The language translations are written in. */
  target: "en" | "sv";
  /** `--trace-timings`: report each drawn update with `lens_drawn`. */
  traceTimings: boolean;
  /** `--debug-dump-frames`: the folder frames are saved to (PRIV-02 red banner). */
  dumpFrames: string | null;
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
  /** Bytes of the Swedish ↔ English models still missing (0 = both installed). */
  swedish_missing_bytes: number;
  swedish_downloadable: boolean;
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
  /** Pre-selected by the first-run wizard. */
  suggested: boolean;
}

export interface JobView {
  ids: string[];
  downloaded: number;
  total: number;
}
