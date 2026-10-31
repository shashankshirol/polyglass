# ADR-0001: Headless core crates and pipeline design

- **Status:** accepted
- **Date:** 2026-10-31
- **Requirements:** CAP-03, LENS-06, TR-04, TR-05, TR-07, OCR-02, PRIV-07, QUAL-01

## Context

PLAN principle 4 asks for all capture/OCR/translation logic in plain Rust crates without Tauri,
so it can be unit-tested and benchmarked from a CLI. The pipeline must never draw stale results,
must do no work while hidden, and must skip work when nothing changed.

## Decision

- Seven crates as in PLAN §3.1; only `apps/desktop` (later) depends on Tauri.
- Engines are trait objects (`Box<dyn ScreenCapturer>`, `Arc<dyn OcrEngine>`,
  `Arc<dyn Translator>`) so the app can swap backends at runtime.
- The `Scheduler` is a pure state machine over `tokio::time::Instant`; the async driver is thin.
  This makes CAP-03/LENS-06 testable with Tokio's paused clock.
- Staleness is handled twice: generation bump drops the in-flight future, and emitted results
  are compared with the current generation.
- Capture + change detection + OCR + grouping run in one `spawn_blocking` call per cycle;
  translation runs on the async runtime.
- Change detection uses an 8×8-px mean-luma grid with a changed-cell fraction threshold.
- Swedish/English identification is a lexicon + morphology scorer, not a model: it only has to
  separate two languages and runs in microseconds.

## Consequences

- Platform engines plug in by implementing the traits; no pipeline changes needed.
- Change-detection thresholds (`ChangeConfig`) need tuning against real captures (caret blink,
  anti-aliasing) once WGC/ScreenCaptureKit capture lands.
- The language identifier is tuned on `tests/fixtures/mixed-sv-en`; a held-out set is needed to
  avoid overfitting (CHECKLIST M0-FIX-1 note).
- An OCR call already running on the blocking pool when the lens moves runs to completion (its
  result is discarded); OCR never *starts* while moving.
