# Architecture

Companion to [PLAN.md §3](../PLAN.md). This describes the code as it exists; PLAN.md describes
the target. v1 is Windows only (D-15); [Adding a platform](#adding-a-platform) lists where
macOS or Linux would plug in.

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
| `pg-ocr` | Pixels → lines | `OcrEngine`, `resolve_language` (OCR-06/SV-02), `MockOcr`, `WindowsOcr` (WinRT `Windows.Media.Ocr`, Windows only) |
| `pg-translate` | Text → text | `Translator`, `TranslateError` (TR-06 kinds + message IDs), `langid` (SV-04/TR-07; Swedish, English and German profiles, any two of them as a pair), `tokens` (SV-05 check), `TokenGuard` (SV-05: masked retry, else source), `PivotTranslator`/`plan_route` (TR-03), `MockTranslator` |
| `pg-ct2` | On-device translation (ADR-0005) | `Ct2Translator` (id `ctranslate2`, local): CTranslate2 through `ct2rs` running OPUS-MT models converted to int8; `load_installed` loads every installed `ctranslate2` model, any pair; `Options` (threads, beam); `split_sentences` |
| `pg-pipeline` | Orchestration | `Scheduler` (CAP-03, LENS-06), `change` (CAP-03/06), `grouping` (OCR-02), `cache` (TR-04), `color` (REN-01), `Pipeline::run_cycle`, `driver::spawn`, `LensUpdate` |
| `pg-models` | Model files | `Manifest` (MDL-01/02; entries without URLs are import-only), `ModelStore` (MDL-04/05), `Downloader` (MDL-03: Range resume, cancel; follows redirects to https URLs only); `import-models` binary (MDL-04) |
| `pg-bench` | Measurement | CLI over `Pipeline::run_cycle` (SPEC §5) on a rendered fixture image (`--scale`, `--theme`); `--ocr mock\|windows`, the latter also prints CER/WER/å-ä-ö for the lines in the lens; `--translator mock\|ct2` (ct2 = the app's backend: installed models behind `TokenGuard`); `--eval <set>` scores a `tests/eval` set with chrF (SV-07) |
| `pg-testkit` | Test helpers | `TextRenderer` (text into a `Frame` with the system UI font); `fixture` (`truth.json` → `FixtureImage` at 100–200%, light/dark, with line boxes; `render-fixtures` binary); `metrics` (CER, WER, å/ä/ö misreadings; chrF as sacreBLEU computes it); `eval` (the `tests/eval` sets, M0-FIX-4) |

## One cycle

`Pipeline::run_cycle(tick)`:

1. **Blocking pool:** capture `tick.region` → `FrameSignature` → skip if unchanged against the
   last processed frame (a caret blink doesn't count, ADR-0003) and not forced → "No text
   found" if blank → OCR (Auto mode prefers the Swedish recognizer, falls back to English with
   a notice) → confidence filter → group lines into blocks → sample bg/fg colour.
2. **Async:** for each block decide its language (`SourceLang::Fixed`, or `SourceLang::Auto`:
   `langid::decide` between the pair's source and target, MDL-07); blocks
   already in the target language pass through (TR-07); look up the LRU cache; send the misses
   in one request per language pair (TR-05); fill the cache.
3. Return a `LensUpdate` with lens-local logical rects, colours, backend info (PRIV-05) and
   stage timings (`tracing` target `pg::timings`).

## The live driver

`driver::spawn` runs a `tokio::select!` loop over: lens commands, the in-flight cycle, and the
scheduler deadline. Any command that bumps the generation drops the in-flight cycle, and
results are compared with the current generation before being emitted, so results for an old
lens position are never drawn. When hidden, the scheduler has no deadline and the loop only
waits on its channel (LENS-06, RES-01). A `show` while already shown changes nothing, so the
app's `show` after each move doesn't cut the 150 ms rest delay short (CAP-03; before session 12
it did, and every move started a cycle).

## Desktop shell (`apps/desktop`)

A thin Tauri 2 layer; no pipeline logic lives here or in JS.

| Module (`src-tauri/src`) | Role |
|---|---|
| `lens.rs` | Creates the frameless, transparent, always-on-top, no-taskbar window and sets `WDA_EXCLUDEFROMCAPTURE` on it before it is shown (CAP-02); centres it on the cursor's monitor (LENS-01); `Layout` (frame/toolbar insets, Panel strip height) and `interior_region` (logical → `LogicalRect::to_physical`, CAP-04); arrow-key nudge (LENS-02). Panel mode (MODE-02): the window grows by a strip below the lens, or above it when the monitor's work area has no room below (`panel_side`, `apply_mode`); the lens itself never moves and the strip is outside the interior, so it isn't captured or click-through. A flip waits for the mouse button to come up after a drag (`follow_panel`). `[lens] mode` in `settings.toml` |
| `freeze.rs` | Freeze mode (MODE-03, Ctrl+Alt+F, ❄): one capture of the monitor under the lens through the pipeline's capturer, held in memory (`Freeze`); `Freezable` wraps the capturer and cuts regions on that monitor from the snapshot. The `freeze` window (`ui/freeze.html`) gets the pixels as raw bytes (`freeze_snapshot`), draws them into a canvas and is shown under the lens once drawn. Esc, ❄ or the hotkey end it; hiding the lens too. The snapshot is dropped then (PRIV-02) |
| `state.rs` | `Lens`: visibility, pause and interior region in one place; forwards them to the `DriverHandle` (`show`/`hide`/`lens_moved`) and to the click-through poller |
| `clickthrough.rs` | ~30 Hz cursor poll toggling `set_ignore_cursor_events` over the interior only; parked while hidden (ADR-0002) |
| `commands.rs` | The only IPC commands. Lens: `lens_config`, `nudge_lens`, `set_click_through`, `set_paused`, `set_mode`, `set_frozen`, `hide_lens`, `open_ocr_language_settings`, `open_model_manager`, `open_settings`, `copy_translation`, `swap_languages`, `set_languages`, `lens_drawn` (`--trace-timings` only). Settings window: `settings_state`, `set_appearance`, `set_hotkey`, `record_hotkeys`, `open_model_manager`, `open_privacy_policy`. Model manager window: `models_state`, `download_models`, `cancel_download`, `delete_model`, `import_models`, `finish_first_run`. Freeze window: `freeze_snapshot`, `freeze_ready`, `set_frozen`. Each checks the calling window; input validated in Rust (SEC-01) |
| `live.rs` | Default on Windows: `WgcCapturer` + `WindowsOcr` + `TokenGuard<Ct2Translator>`, Auto sv/en → en. Started only if the lens is excluded from capture. Without models, Swedish blocks report `tr-error-model-missing` |
| `models.rs` | Model store at `<app local data>/models` (`%LOCALAPPDATA%\org.polyglass.desktop\models` on Windows) and the `models` window (`ui/models.html`): the model manager and, on the first live start without the sv↔en models, the first-run wizard (the selected pair's models, by default both sv↔en directions, pre-selected with their combined size; offered once, `[models] first_run_done`; MDL-06, SV-01). The manager lists the catalog, Swedish first, with size, license, attribution and source (MDL-01); one download job at a time through `pg-models` with progress, cancel and resume from `.part` files (MDL-03); delete and total disk usage (MDL-05). After a download or delete the running pipeline gets a translator with exactly the installed models. Import-only entries, and every download after one got HTTP 401/403/404/410 in this run (`mdl-error-unreachable`, e.g. while the repository is private), show the `scripts/models/opus_mt.py` command instead of a download button. The lens ⬇ (shown while the models are missing) and the Settings window open it, as does the tray's "Translation models…" |
| `hotkeys.rs` | Global hotkeys through `tauri-plugin-global-shortcut` (registered from Rust only): the HK-01 defaults, `[hotkeys]` in `settings.toml` (`Ctrl+Alt+L`, "" = off) and rebinding (HK-02). A new binding is refused with a `Problem` when another action has it, when Ctrl+Alt (= AltGr) with that key types a character on an installed keyboard layout (`altgr_character`, `ToUnicodeEx`, SV-09), or when registering fails (another app or Windows holds it); the old binding then stays. While the Settings window records keys, every hotkey is unregistered. Saved hotkeys that fail to register at start are reported as not working |
| `prefs.rs` | The `settings` window (`ui/settings.html`), opened by ⚙ and the tray's "Settings…": appearance (LENS-07), hotkeys (HK-02) and a button to the model manager. Every value is checked here before it is saved and sent to the lens |
| `appearance.rs` | `[appearance]` in `settings.toml` (LENS-07): frame colour (`#rrggbb`), overlay opacity 20–100 % (the blocks' backgrounds; text stays opaque), a font from a fixed list of fonts that ship with Windows, font scale 75–200 % (scales REN-02's start and minimum size and the Panel text). `set_appearance` rejects values out of range; a hand-edited file is brought into range. The lens picks light or dark toolbar text for the frame colour (`ui/src/appearance.ts`, ≥ 4.5:1). Reduced motion needs no setting: every stylesheet turns animations off under `prefers-reduced-motion`, which WebView2 takes from Windows' Animation effects |
| `demo.rs` | `--demo`: `StaticCapturer` over a synthetic desktop + `MockOcr` + `MockTranslator` |
| `tray.rs` | Tray icon: show/hide lens, Settings, translation models, crash reports (when there are any), quit |
| `langs.rs` | The language pair (MDL-07): every pair in the model catalog can be picked in the toolbar; the choice is `[translation] source`/`target`/`auto` in `settings.toml`, made valid against the catalog at start. `apply` saves a choice, sets the pipeline's `SourceLang` and target, tells `models.rs` which models to offer (the pair and its reverse) and sends `lens-languages` to the lens. Nothing names a language except the SV-01 default |
| `devflags.rs` | Developer flags (M1-APP-10): `--trace-timings[=uncached]` (`Trace`: stage timings per cycle, and at rest → drawn once the lens reports the paint through `lens_drawn`; target `pg::trace`, numbers only) and `--debug-dump-frames[=DIR]` (`DumpFrames` wraps the capturer and a writer thread saves PNGs; the lens shows a red banner, PRIV-02) |
| `privacy.rs` | The built-in `PRIVACY.md` (OSS-03): Settings' "Privacy policy…" saves it as `<app local data>/PRIVACY.txt` and opens it in the default text editor; nothing from the webview reaches the path |
| `crash.rs` | Panic hook: a plain-text report per panic in `<app local data>/crash-reports`, newest 10 kept, nothing uploaded (PRIV-06) |

Security: `build.rs` declares the commands in an app manifest so each needs an explicit
`allow-*` permission; `capabilities/lens.json` grants the lens window only event listen/unlisten,
start-dragging/resize-dragging and the lens commands, and `capabilities/models.json` grants the
model manager window only event listen/unlisten and the model commands, `capabilities/settings.json`
the Settings window only event listen/unlisten and its commands — no `core:default`.
Every command also checks the label of the window that called it. The CSP in
`tauri.conf.json` allows bundled scripts/styles only, no frames, objects or remote connections
besides Tauri IPC (SEC-02). The UI renders recognized text with `textContent` only and accepts
colours only as `#rrggbb`.

Events: the driver's `LensUpdate`s are emitted to the lens window as `lens-update` (JSON as in
`pg_pipeline::update`) and drawn as absolutely positioned DOM blocks in the interior. If the
pipeline can't start, `lens-error` carries a message ID (also in `lens_config`).
`models-changed` carries a `ModelsView` (catalog rows with install state, disk usage, the running
job's progress, the last outcome's message ID) to both windows whenever it changes.

`unsafe`: the workspace denies it. Allowed only in `pg-capture`'s `wgc` module (D3D11/DXGI
interop, monitor enumeration), the one `SetWindowDisplayAffinity` call in `lens.rs`, the key and
mouse-button state reads in `hold.rs`, the keyboard-layout reads in `hotkeys.rs` (AltGr check), and the
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
- Frames reach the disk only through `--debug-dump-frames`, with the lens's red banner on
  (PRIV-02). The pipeline's logs hold no text even at TRACE (`pg-pipeline/tests/priv07_logs.rs`,
  PRIV-07); `--trace-timings` logs numbers only.
- Crash reports stay on the computer (`crash.rs`, PRIV-06); there is no telemetry code.
- `TranslationCache` has no serialization (TR-04: never on disk).
- Errors carry technical context only, never recognized text.
- Model downloads accept only manifest entries; the manifest accepts only https URLs with
  SHA-256 (MDL-02, SEC-03). The app downloads only when the user presses a download button in the model manager or
  the first-run wizard (MDL-06). Imported
  models are checked against the same SHA-256 (MDL-04).
- Translation stays on the device with `pg-ct2` (`is_local() == true`, PRIV-05).

## Adding a platform

v1 ships on Windows only (D-15). Everything OS-specific sits behind a trait or a `cfg(windows)`
item, and the rest (`pg-core`, `pg-translate`, `pg-ct2`, `pg-pipeline`, `pg-models`, the UI) has
no platform code. The non-Windows branches return "unsupported" or nothing, so the app would
start in demo mode elsewhere; no CI job builds it there, so expect some rot. To add macOS or
Linux:

| Where | What to add |
|---|---|
| `pg-capture` | A `ScreenCapturer` for the platform in its own `#[cfg]` module, next to `wgc` (SPEC CAP-01/02/05; PLAN §4–5 has the chosen APIs) |
| `pg-ocr` | An `OcrEngine` in its own `#[cfg]` module, next to `winrt`; if a platform needs different engines per language (macOS: Vision has no Swedish), add a selection function (SPEC OCR-01) |
| `apps/desktop/src-tauri` | `live.rs` `pipeline()` (pick the capturer and engine), `lens.rs` `exclude_from_capture`, `hold.rs` `keys()` (REN-04), `commands.rs` `open_ocr_language_settings`, `crash.rs` `open_folder`, `privacy.rs` `open`, `hotkeys.rs` `modifiers()` (⌘⌥ on macOS); Tauri's `macos-private-api` feature and `macOSPrivateApi` for transparent windows on macOS; `tauri.conf.json` bundle targets and icons |
| Paths | The model store path in `pg-bench` (`app_model_store`) and `import-models` (`app_store`) is `%LOCALAPPDATA%` only; `pg-testkit`'s system fonts are Windows fonts |
| Build and CI | `ci.yml` and `measure.yml` (a job per OS), `deny.toml` `[graph] targets`, and the platform notes in CHECKLIST.md "Post-v1 platforms" (e.g. Linux needs `ct2rs`'s `disable-cpu-dispatch`) |
