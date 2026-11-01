# Architecture

Companion to [PLAN.md §3](../PLAN.md). This describes the code as it exists; PLAN.md describes
the target.

## Crates

```
pg-core ◄── pg-capture ◄──┐
   ▲   ◄── pg-ocr     ◄──┼── pg-pipeline ◄── pg-bench
   │   ◄── pg-translate ◄┘                   apps/desktop
   │          ▲                                  │
   │          └── pg-ct2 ◄───────────────────────┘
   └────── pg-models ◄┘

pg-testkit (dev-only: fixture images and OCR scoring for tests and pg-bench)
```

| Crate | Owns | Key items |
|---|---|---|
| `pg-core` | Types shared by everything | `PhysRect`, `LogicalRect::to_physical` (CAP-04), `Frame` (redacted `Debug`), `Line`, `Block::text` (de-hyphenation), `LangTag`, `LangPair`, `SourceLang`, `Error` |
| `pg-capture` | Getting pixels | `ScreenCapturer` (incl. `release` while hidden), `StaticCapturer`, `FixtureCapturer` (PNG file or bytes), `WgcCapturer` + `monitors()` (Windows Graphics Capture, Windows only; ADR-0003) |
| `pg-ocr` | Pixels → lines | `OcrEngine`, `default_engine` (OCR-01 table), `resolve_language` (OCR-06/SV-02), `MockOcr`, `WindowsOcr` (WinRT `Windows.Media.Ocr`, Windows only) |
| `pg-translate` | Text → text | `Translator`, `TranslateError` (TR-06 kinds + Fluent IDs), `langid` (SV-04/TR-07), `tokens` (SV-05 check), `TokenGuard` (SV-05: masked retry, else source), `PivotTranslator`/`plan_route` (TR-03), `MockTranslator` |
| `pg-ct2` | On-device translation (ADR-0005) | `Ct2Translator` (id `ctranslate2`, local): CTranslate2 through `ct2rs` running OPUS-MT models converted to int8; `load_installed` loads every installed `ctranslate2` model, any pair; `Options` (threads, beam); `split_sentences` |
| `pg-pipeline` | Orchestration | `Scheduler` (CAP-03, LENS-06), `change` (CAP-03/06), `grouping` (OCR-02), `cache` (TR-04), `color` (REN-01), `Pipeline::run_cycle`, `driver::spawn`, `LensUpdate` |
| `pg-models` | Model files | `Manifest` (MDL-01/02; entries without URLs are import-only), `ModelStore` (MDL-04/05), `Downloader` (MDL-03: Range resume, cancel; follows redirects to https URLs only); `import-models` binary (MDL-04) |
| `pg-bench` | Measurement | CLI over `Pipeline::run_cycle` (SPEC §5) on a rendered fixture image (`--scale`, `--theme`); `--ocr mock\|windows`, the latter also prints CER/WER/å-ä-ö for the lines in the lens |
| `pg-testkit` | Test helpers | `TextRenderer` (text into a `Frame` with the system UI font); `fixture` (`truth.json` → `FixtureImage` at 100–200%, light/dark, with line boxes; `render-fixtures` binary); `metrics` (CER, WER, å/ä/ö misreadings) |

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
| `lens.rs` | Creates the frameless, transparent, always-on-top, no-taskbar window and sets `WDA_EXCLUDEFROMCAPTURE` on it before it is shown (CAP-02); centres it on the cursor's monitor (LENS-01); `Layout` (frame/toolbar insets) and `interior_region` (logical → `LogicalRect::to_physical`, CAP-04); arrow-key nudge (LENS-02) |
| `state.rs` | `Lens`: visibility, pause and interior region in one place; forwards them to the `DriverHandle` (`show`/`hide`/`lens_moved`) and to the click-through poller |
| `clickthrough.rs` | ~30 Hz cursor poll toggling `set_ignore_cursor_events` over the interior only; parked while hidden (ADR-0002) |
| `commands.rs` | The only IPC commands (`lens_config`, `nudge_lens`, `set_click_through`, `set_paused`, `hide_lens`, `open_ocr_language_settings`, `download_models`); input validated in Rust (SEC-01) |
| `live.rs` | Default on Windows: `WgcCapturer` + `WindowsOcr` + `TokenGuard<Ct2Translator>`, Auto sv/en → en. Started only if the lens is excluded from capture. Without models, Swedish blocks report `tr-error-model-missing` |
| `models.rs` | Model store at `<app local data>/models` (`%LOCALAPPDATA%\org.polyglass.desktop\models` on Windows). When the sv↔en models are missing, the toolbar ⬇ (`download_models`) downloads them from the GitHub release (D-9) through `pg-models`, reports `lens-models` progress events, then swaps the loaded translator into the running pipeline. For an import-only entry (no URLs) `lens_config.modelsDownloadable` is false and the lens points to `scripts/models/opus_mt.py` instead; the same happens for the rest of the run after a download gets HTTP 401/403/404/410 (`mdl-error-unreachable`, e.g. while the repository is private). Other failures keep ⬇ for a retry. Interim until M1-APP-8 |
| `demo.rs` | `--demo`: `StaticCapturer` over a synthetic desktop + `MockOcr` + `MockTranslator` |
| `tray.rs` | Tray icon: show/hide lens, quit |

Security: `build.rs` declares the commands in an app manifest so each needs an explicit
`allow-*` permission; `capabilities/lens.json` grants the lens window only event listen/unlisten,
start-dragging/resize-dragging and those seven commands — no `core:default`. The CSP in
`tauri.conf.json` allows bundled scripts/styles only, no frames, objects or remote connections
besides Tauri IPC (SEC-02). The UI renders recognized text with `textContent` only and accepts
colours only as `#rrggbb`.

Events: the driver's `LensUpdate`s are emitted to the lens window as `lens-update` (JSON as in
`pg_pipeline::update`) and drawn as absolutely positioned DOM blocks in the interior. If the
pipeline can't start, `lens-error` carries a Fluent message ID (also in `lens_config`).
`lens-models` carries model-download progress (`downloading` / `ready` / `error`).

`unsafe`: the workspace denies it. Allowed only in `pg-capture`'s `wgc` module (D3D11/DXGI
interop, monitor enumeration), the one `SetWindowDisplayAffinity` call in `lens.rs`, and the
Win32 test windows in `crates/pg-capture/tests/wgc.rs`. `WindowsOcr` and `pg-ct2` need none:
CTranslate2 is C++, and its `cxx` bridge (with its `unsafe`) lives inside `ct2rs`.

## On-device translation (`pg-ct2`, ADR-0005)

- Engine: CTranslate2 (OpenNMT) built from source by `ct2rs` with CMake (ruy CPU backend,
  SentencePiece tokenizer). On Windows it links the static MSVC runtime, so
  `.cargo/config.toml` sets `+crt-static` for MSVC targets, and it selects the Ninja generator
  (cmake-rs drops the optimization flags with the Visual Studio generator).
- Models: `opus-mt-sv-en` and `opus-mt-en-sv` in `models/manifest.toml` (Helsinki-NLP OPUS-MT,
  CC-BY-4.0 with attribution, 84.8 MB each). Each is a CTranslate2 directory: `model.bin`
  (int8), `config.json`, `shared_vocabulary.json`, `source.spm`, `target.spm` and the
  `LICENSE`. They're produced by `scripts/models/opus_mt.py` (pinned converter, pinned input and
  output SHA-256). They're hosted as assets of the GitHub pre-release `models-opus-mt-2026-11-01`
  (D-9), named `<id>.<file>` because a release's assets share one namespace;
  `.github/workflows/models-release.yml` builds and publishes a release when a
  `models-opus-mt-*` tag is pushed (`scripts/models/release_assets.py` checks the manifest URLs
  and hashes and writes the license, attribution and notes). The same script installs them
  offline by import (MDL-04). Nothing in
  `pg-ct2` names a language: another pair (M3-8/M3-9) is a manifest entry plus a script entry.
- Every installed direction is loaded at startup; swapping direction never reloads (SV-01).
  Each model uses `Options::default_threads()` threads (half the CPUs, at most 4). Translation
  runs on Tokio's blocking pool: each block is split into sentences (OPUS-MT is sentence-level)
  and all sentences of a request go to CTranslate2 as one batch, greedy decoding.
- `TokenGuard` (SV-05): blocks whose output fails `tokens::check_preserved` are translated
  again with URLs, e-mails and number runs masked as `{n}`. Restored numbers are reformatted
  only where the verbatim form would read as a different value in the target language. If no
  pass keeps the tokens, the source text is shown.
- Real-model tests (`crates/pg-ct2/tests/real_models.rs`) are `#[ignore]`d because they need
  the converted models; nothing in `cargo test --workspace` touches the network.

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

## Fixtures and accuracy tests

- Fixture images are rendered from `tests/fixtures/*/truth.json` when a test or `pg-bench` needs
  them (`pg_testkit::fixture`), never committed; layout, fonts and why are in
  `tests/fixtures/README.md`. Rendering is deterministic per font file.
- They reach code under test as PNG bytes through `FixtureCapturer::from_png`, i.e. the same
  path as an image file.
- `pg_testkit::metrics::score` compares recognized text with the rendered lines after NFC and
  whitespace normalization: CER and WER by Levenshtein distance, plus an alignment-based count of
  how each reference å/ä/ö/Å/Ä/Ö was misread (SV-03). Scores add up across images.
- Tests: `crates/pg-capture/tests/fixture.rs` (CAP-04: lens regions at 100/125/150/200%),
  `crates/pg-ocr/tests/accuracy.rs` (OCR-04/SV-03 gates on Windows OCR; they skip with a message
  when the font or an OCR language is missing, unless `POLYGLASS_REQUIRE_OCR_GATES=1`).

## Privacy invariants in code

- `Frame` and `RenderedBlock` have hand-written `Debug` impls without pixels/text (PRIV-02/07).
- `TranslationCache` has no serialization (TR-04: never on disk).
- Errors carry technical context only, never recognized text.
- Model downloads accept only manifest entries; the manifest accepts only https URLs with
  SHA-256 (MDL-02, SEC-03). The app downloads only when the user presses ⬇ (MDL-06). Imported
  models are checked against the same SHA-256 (MDL-04).
- Translation stays on the device with `pg-ct2` (`is_local() == true`, PRIV-05).
