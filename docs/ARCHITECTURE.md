# Architecture

Companion to [PLAN.md §3](../PLAN.md). This describes the code as it exists; PLAN.md describes
the target.

## Crates

```
pg-core ◄── pg-capture ◄──┐
   ▲   ◄── pg-ocr     ◄──┼── pg-pipeline ◄── pg-bench
   │   ◄── pg-translate ◄┘                   apps/desktop
   └────── pg-models

pg-testkit (dev-only: text rendering for tests and pg-bench)
```

| Crate | Owns | Key items |
|---|---|---|
| `pg-core` | Types shared by everything | `PhysRect`, `LogicalRect::to_physical` (CAP-04), `Frame` (redacted `Debug`), `Line`, `Block::text` (de-hyphenation), `LangTag`, `LangPair`, `SourceLang`, `Error` |
| `pg-capture` | Getting pixels | `ScreenCapturer`, `StaticCapturer`, `FixtureCapturer` |
| `pg-ocr` | Pixels → lines | `OcrEngine`, `default_engine` (OCR-01 table), `resolve_language` (OCR-06/SV-02), `MockOcr`, `WindowsOcr` (WinRT `Windows.Media.Ocr`, Windows only) |
| `pg-translate` | Text → text | `Translator`, `TranslateError` (TR-06 kinds + Fluent IDs), `langid` (SV-04/TR-07), `tokens` (SV-05), `PivotTranslator`/`plan_route` (TR-03), `MockTranslator` |
| `pg-pipeline` | Orchestration | `Scheduler` (CAP-03, LENS-06), `change` (CAP-03/06), `grouping` (OCR-02), `cache` (TR-04), `color` (REN-01), `Pipeline::run_cycle`, `driver::spawn`, `LensUpdate` |
| `pg-models` | Model files | `Manifest` (MDL-01/02), `ModelStore` (MDL-04/05), `Downloader` (MDL-03) |
| `pg-bench` | Measurement | CLI over `Pipeline::run_cycle` (SPEC §5); `--ocr mock\|windows` |
| `pg-testkit` | Test helpers | `TextRenderer`: draws text into a `Frame` with an installed system font (until fixture images exist, M0-FIX-2) |

## One cycle

`Pipeline::run_cycle(tick)`:

1. **Blocking pool:** capture `tick.region` → `FrameSignature` → skip if unchanged and not
   forced → "No text found" if blank → OCR (Auto mode prefers the Swedish recognizer, falls back
   to English with a notice) → confidence filter → group lines into blocks → sample bg/fg colour.
2. **Async:** for each block decide its language (fixed, or `langid::decide_sv_en`); blocks
   already in the target language pass through (TR-07); look up the LRU cache; send the misses
   in one request per language pair (TR-05); fill the cache.
3. Return a `LensUpdate` with lens-local logical rects, colours, backend info (PRIV-05) and
   stage timings (`tracing` target `pg::timings`).

## The live driver

`driver::spawn` runs a `tokio::select!` loop over: lens commands, the in-flight cycle, and the
scheduler deadline. Any command that bumps the generation drops the in-flight cycle, and
results are compared with the current generation before being emitted, so results for an old
lens position are never drawn. When hidden, the scheduler has no deadline and the loop only
waits on its channel (LENS-06, RES-01).

## Desktop shell (`apps/desktop`)

A thin Tauri 2 layer; no pipeline logic lives here or in JS.

| Module (`src-tauri/src`) | Role |
|---|---|
| `lens.rs` | Creates the frameless, transparent, always-on-top, no-taskbar window; centres it on the cursor's monitor (LENS-01); `Layout` (frame/toolbar insets) and `interior_region` (logical → `LogicalRect::to_physical`, CAP-04); arrow-key nudge (LENS-02) |
| `state.rs` | `Lens`: visibility, pause and interior region in one place; forwards them to the `DriverHandle` (`show`/`hide`/`lens_moved`) and to the click-through poller |
| `clickthrough.rs` | ~30 Hz cursor poll toggling `set_ignore_cursor_events` over the interior only; parked while hidden (ADR-0002) |
| `commands.rs` | The only IPC commands (`lens_config`, `nudge_lens`, `set_click_through`, `set_paused`, `hide_lens`); input validated in Rust (SEC-01) |
| `demo.rs` | `--demo`: `StaticCapturer` over a synthetic desktop + `MockOcr` + `MockTranslator` |
| `tray.rs` | Tray icon: show/hide lens, quit |

Security: `build.rs` declares the commands in an app manifest so each needs an explicit
`allow-*` permission; `capabilities/lens.json` grants the lens window only event listen/unlisten,
start-dragging/resize-dragging and those five commands — no `core:default`. The CSP in
`tauri.conf.json` allows bundled scripts/styles only, no frames, objects or remote connections
besides Tauri IPC (SEC-02). The UI renders recognized text with `textContent` only and accepts
colours only as `#rrggbb`.

Events: the driver's `LensUpdate`s are emitted to the lens window as `lens-update` (JSON as in
`pg_pipeline::update`) and drawn as absolutely positioned DOM blocks in the interior.

## Windows OCR (`pg-ocr::WindowsOcr`)

- Languages come from `OcrEngine.AvailableRecognizerLanguages` on every call that names a
  language (OCR-06); a missing one is `Error::OcrLanguageMissing` (SV-02). Recognizers are cached
  per language tag.
- The frame is converted to packed BGRA, enlarged 2× (bilinear) below 200% scale, and copied
  into a `SoftwareBitmap` via a WinRT `DataWriter` buffer: no `unsafe`. Word boxes are mapped
  back to frame pixels; a line's box is the union of its words. Confidence is always 1.0
  (Windows OCR has none).
- The blocking call `IAsyncOperation::get()` runs on the pipeline's blocking pool. windows-rs
  initialises the thread's COM apartment (MTA) on first use.

## Privacy invariants in code

- `Frame` and `RenderedBlock` have hand-written `Debug` impls without pixels/text (PRIV-02/07).
- `TranslationCache` has no serialization (TR-04: never on disk).
- Errors carry technical context only, never recognized text.
- Model downloads accept only manifest entries; the manifest accepts only https URLs with
  SHA-256 (MDL-02, SEC-03).
