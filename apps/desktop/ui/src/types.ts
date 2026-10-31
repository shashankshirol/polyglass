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
  /** False while the missing models are import-only (not hosted yet). */
  modelsDownloadable: boolean;
}

/** `models::ModelEvent`: progress of the toolbar's model download. */
export type ModelEvent =
  | { state: "downloading"; downloaded: number; total: number }
  | { state: "ready" }
  | { state: "error"; message_id: string };
