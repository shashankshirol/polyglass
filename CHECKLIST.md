# Polyglass — Development Checklist

> Living tracker for implementation work. Companion to [`SPEC.md`](./SPEC.md) (requirements) and [`PLAN.md`](./PLAN.md) (architecture/roadmap).
> **Every session:** read "Current state" and the latest "Session log" entry first, pick tasks, mark them `[~]`, and add a log entry before you finish.

## How to use this file

| Mark | Meaning |
|---|---|
| `[ ]` | Not started |
| `[~]` | In progress / partially done (see note) |
| `[x]` | Done and verified (tests pass or reviewer checked) |
| `[!]` | Blocked (reason in note) |
| `[-]` | Dropped / deferred past v1 (reason in note) |

- Task IDs are `M<milestone>-<area>-<n>` and never reused. Requirement IDs (e.g. `CAP-03`) point into SPEC.md.
- A task is `[x]` only when its acceptance check is automated (test name given) or noted as manually verified.
- Keep tasks small enough for one session. Split a task instead of leaving it `[~]` for several sessions.
- Commands every session should keep green:
  ```
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

## Current state

- **Milestone:** M1. Headless core done. On Windows the lens runs **live by default**: Windows Graphics Capture of the lens interior + Windows OCR + **on-device translation** (`pg-bergamot`: Mozilla's Firefox Translations sv↔en models on the pure-Rust `fxtranslate` engine, behind the SV-05 `TokenGuard`; ADR-0004). Models aren't bundled: without them the lens says so and the toolbar ⬇ downloads them (30.5 MB). `--demo` / `POLYGLASS_DEMO=1` keeps all mocks. Fixture images are rendered from `truth.json` at test time (100–200%, light/dark) and Windows OCR is gated on them: OCR-04 and the SV-03 CER part pass, the SV-03 å/ä/ö part fails (M1-OCR-6). 151 tests green (5 ignored: 1 measurement, 3 real-model tests that download, the failing SV-03 letter gate), clippy `-D warnings` and fmt clean (Windows only so far). ADR-0002 (click-through), ADR-0003 (capture) and ADR-0004 (translation) are verified on Windows only.
- **Dev machine used so far:** Windows 11 x64, Rust 1.99, Node 24, Python 3.12 (no Pillow), no Tesseract. Windows OCR languages installed: `en-US`, `en-GB`, `sv-SE` (Swedish was added after session 2; the real "Swedish missing" path is now tested with an uninstalled tag instead).
- **Monitors on this machine:** 2560×1440 at 100% (primary) and 2160×3840 at 150% (above-left, negative coordinates), which is useful for CAP-04 and mixed-DPI checks.
- **Git:** local repo on `main`; no remote yet.
- **Next up (suggested order, each ≈ one session):**
  1. M1-OCR-6 rest: SV-03 å/ä/ö fails on Windows OCR (7.3% vs ≤ 2%; ä→å/ä→a at 100–125%). First find out whether the renderer causes it: draw the fixtures with GDI/DirectWrite ClearType in a real window and capture with WGC (as `crates/pg-capture/tests/wgc.rs` does), then score with `pg_testkit::metrics`. If real text fails too, try mitigations (frame padding, other upscaling, Tesseract `swe` on Windows) and/or bring the result to the owner.
  2. Tune `ChangeConfig` and measure CPU on real captures in a release build (ADR-0003 "Not yet verified").
  3. M1-APP-6..9 (font fitting, hold-to-show-original, settings TOML + a model manager replacing the toolbar ⬇, hotkeys + the swap button now that both directions are loaded).
  4. M0-FIX-4 eval sets, then a `pg-bench` translation stage with the real backend (PERF-01/06 end to end).
  5. M0-SPK-1/SPK-2/SPK-3 on macOS/Linux.
- **Known follow-ups inside done work** (not separate tasks yet):
  - `ChangeConfig` thresholds (8 px cells, 6-level tolerance, 0.15% of cells) are only tested on synthetic frames. Tune on real captures (caret blink, ClearType).
  - `langid` scores 100% on `mixed-sv-en`, but it was tuned on that same fixture. Add a held-out set before trusting the SV-04 ≥ 90% gate.
  - `reqwest` uses rustls with the default **aws-lc-rs** provider. It built fine here, but check its binary size (SIZE-01) and build needs on macOS/Linux/arm64; switch to `ring` if needed.
  - An OCR call already running when the lens moves runs to completion and its result is discarded. OCR never *starts* while moving (ADR-0001).
  - The pipeline queries OCR languages through `recognize` errors on each cycle, and `WindowsOcr` queries `AvailableRecognizerLanguages` on each `recognize` that names a language. That is inside the ≈19 ms OCR p50, so there is no cache yet.
  - `WindowsOcr` reports confidence 1.0 for every line (Windows OCR has none), so the OCR-05 threshold never hides Windows OCR lines.
  - Capture recovery after monitor hot-plug, resolution change or device loss is untested (ADR-0003); errors surface as `cap-error`.
  - The WGC tests (`crates/pg-capture/tests/wgc.rs`) open real windows and need a desktop session with WGC, so they may fail on GitHub's Windows runners. Decide between skipping and a self-hosted runner when CI first runs.
  - Live-mode CPU with the lens visible over static content: 438 ms per 10 s in a debug build (4 Hz capture + change detection + click-through poller). Measure a release build.
  - Tauri is on the 2.12 line since session 5 (D-6: MSRV 1.90): crate 2.12.5 (`tauri = "2.12"`, `tauri-build = "2.7"`), npm `@tauri-apps/api` / CLI 2.12.4. Keep npm and crate on the same minor version when moving on (the CLI warns otherwise). Tauri 2.12 uses `windows` 0.62, so the workspace moved to 0.62 too (one copy).
  - The click-through poller doesn't check mouse-button state (ADR-0002); a drag-selection that crosses into the interior flips to click-through mid-gesture.
  - Demo mode's synthetic desktop uses the starting monitor's scale for all monitors (mock only).
  - The MSRV (1.90 since D-6) is checked locally with `cargo +1.90 check --workspace --all-targets`; CI doesn't run it yet. `pg-capture` is declared with `default-features = false` in `[workspace.dependencies]` (Cargo 1.85 rejected members turning them off) and `pg-bench` asks for `fixture`.
  - `fxtranslate` (ADR-0004) is young (first release 2026-07, one maintainer). Re-run `cargo test -p pg-bergamot --release --test real_models -- --ignored --test-threads 1` on every upgrade. Bergamot C++ (option A) is the documented fallback for the same model files and was **not built** (blocked in session 4).
  - Windows runs fxtranslate's scalar kernel (its AVX2 shim doesn't compile with MSVC): ≈ 7 ms per short string, ≈ 95 ms per two-sentence paragraph. Fix upstream if PERF-01 gets tight.
  - The toolbar ⬇ download has no cancel button or retry backoff (pg-models supports cancel), and a model that fails to load is logged and shown as "missing".
  - `pg-bench` still uses `MockTranslator`, so PERF-01/06 aren't measured end to end. The live app logged an uncached cycle of 31 ms (OCR 8.5 + translation 21.5 ms) for one Swedish sentence.
  - Windows OCR drops isolated short words: "Save" alone at 12 px is missed at 100% even with page margins, and in a crop with ≤ 1/3 of a line gap above and below, "Save" and "Save changes?" vanish at 200% too. 8–16 px of background padding around the frame brought the 200% case to 0% CER (session 6). Consider padding frames inside `WindowsOcr` (the lens edge can sit close to text).
  - Fixture images are grayscale anti-aliased with no hinting (`ab_glyph`). Real screen text uses hinting and ClearType, so OCR numbers on fixtures may be better or worse than on real captures (see Next up 1).
  - The OCR accuracy gates (`crates/pg-ocr/tests/accuracy.rs`) and the Windows OCR part of `crates/pg-capture/tests/fixture.rs` skip with a message when the system font or an OCR language (`en`, `sv-SE`) is missing. GitHub's Windows runners probably lack `sv-SE`, so SV-03 would skip there; set `POLYGLASS_REQUIRE_OCR_GATES=1` on a runner that has Swedish.
  - Stale notes outside session 6's items: M0-CORE-2 still says FixtureCapturer is untested (it is tested now, `crates/pg-capture/tests/fixture.rs`) and M0-FIX-1 lists images as not done (they're rendered at test time; `bbox` stays unused, see D-7).
  - `cargo deny check` passes in full (cargo-deny 0.20.2, session 5) with two ignores in `deny.toml` (D-6): RUSTSEC-2024-0370 `proc-macro-error` (Linux GTK 3 stack of Tauri 2) and RUSTSEC-2026-0192 `ttf-parser` (via `ab_glyph` in `pg-testkit`, dev/bench only). Drop them when Tauri moves off GTK 3 / `ab_glyph` drops ttf-parser.

## Owner decisions pending (PLAN §11, SPEC §10)

- [x] D-1 Final project name. **Decided (2026-11-01): "Polyglass is good for now"**; the `pg-` crate prefix stays.
- [x] D-2 License. **Decided (2026-11-01): "Use the APACHE-2.0 license"**. Done in session 7: `LICENSE` (Apache-2.0 text), workspace `license = "Apache-2.0"`, `apps/desktop/package.json`, README, PLAN §8. `THIRD_PARTY_NOTICES` via `cargo-about` is still M5-2.
- [x] D-3 Minimum OS versions. **Decided (2026-11-01): the proposed versions (Win10 2004, macOS 13, Ubuntu 22.04); "I don't care about supporting old OSes".** Recorded in SPEC §4.
- [-] D-4 Code-signing budget (Apple Developer Program, SignPath). **Deferred by the owner (2026-11-01)**; revisit before the first public beta (M5-1).
- [x] D-5 SPEC §10 open questions 1–6. **Decided (2026-11-01)**, written into SPEC v0.3:
  1. Default direction **Swedish → English**, "for now"; keep pairs modular so users can set up their own languages (SV-01, new MDL-07 → M3-8).
  2. Next pair **German → English** (main audience is English speakers) (§3 → M3-9).
  3. **US** English by default (SV-10 → M3-2).
  4. **Panel** is the default mode on Linux X11 (MODE-02 → M2-2).
  5. On-device is the **fallback when an online backend fails, with a notice** to the user; on-device is the default at launch and online backends are opt-in (TR-06, TR-02 → M3-4).
  6. **No** Apple Translation framework: same backend on every OS (§8).
- [x] D-6 `cargo deny` advisories that MSRV 1.85 / Tauri 2.11 block (session 5). **Decided: "Raise MSRV to 1.90"** — done in session 5 (MSRV 1.90, Tauri 2.12, `cargo update`; quick-xml, time and `unic-*` are gone; the two unmaintained crates below are ignored in `deny.toml` with reasons). Original findings:
  - RUSTSEC-2026-0194, RUSTSEC-2026-0195 (quick-xml 0.38.4, XML DoS) via `plist` 1.8.0 ← tauri-utils/tauri-macros/the app. `plist` ≥ 1.9 drops quick-xml but needs Rust 1.88. Tauri uses it to read our own config/Info.plist at build time; `tauri` itself depends on it only on macOS.
  - RUSTSEC-2026-0009 (time 0.3.45, RFC 2822 parse stack exhaustion) via `cookie` and `plist`. Fixed in time 0.3.47, which needs Rust 1.88.
  - RUSTSEC-2025-0075/0080/0081/0098/0100 (unmaintained `unic-*`) via `urlpattern` 0.3 ← tauri-utils 2.9. Tauri 2.12 (tauri-utils 2.10, urlpattern 0.6) drops them but needs Rust 1.90.
  - RUSTSEC-2024-0370 (unmaintained `proc-macro-error`) via gtk-rs 0.18 ← tray-icon/wry/tauri, Linux only. No fix in Tauri 2.x (GTK 3 bindings).
  - RUSTSEC-2026-0192 (unmaintained `ttf-parser` 0.25) via `ab_glyph` 0.2.32 (latest) ← `pg-testkit`, dev/bench only, never in the app. No fix upstream.
  - Raising the MSRV to 1.88 clears the three vulnerabilities; 1.90 (Tauri 2.12, npm to 2.12.x too) also clears `unic-*`. The two unmaintained crates need ignores either way.
- [ ] D-7 Commit fixture PNGs? (session 6, not blocking.) Today they are rendered at test time with the OS UI font and nothing is committed (`tests/fixtures/README.md`). Segoe UI (Windows) and Arial (Windows/macOS) are licensed with the OS; the font files can't be redistributed, and whether renderings of them can be released under CC0 with the rest of the fixtures is unclear. Options: (a) keep rendering at test time (current); (b) commit PNGs rendered with an OFL font (e.g. Noto Sans) kept in or pinned by the repo — identical images on every OS and CI, but not the Windows UI font; (c) commit Segoe UI renderings after a licensing check. (b) or (c) would fill `image`/`bbox` in `truth.json` (≈ 3 MB for all 64 images).
- [ ] D-8 Translation engine: keep `fxtranslate` (ADR-0004 option C) or move to a more proven runtime? Raised by the owner on 2026-11-01: `fxtranslate` is young, and `browsermt/bergamot-translator` looks stale. Research on 2026-11-01:
  - The Bergamot C++ engine now lives in `mozilla/translations/inference`. Its commits run to 2026-01 but are mostly dependency bumps, and Firefox uses its WASM build, not the native one.
  - CTranslate2 (OpenNMT) is actively released (v4.8.4, 2026-10) and converts MarianMT/OPUS-MT, M2M-100 and NLLB. Session 4 measured it with OPUS-MT on this machine: 26.5 ms short, 199 ms paragraph, 200 MB RSS, 162 MB of models for both directions, CC-BY-4.0, which we'd self-host after conversion. The `ct2rs` binding has one maintainer.
  - `facebook/m2m100_418M` (MIT, 100 languages incl. sv/de/en): 1.94 GB in fp32, roughly 0.4–0.5 GB as int8 (an estimate). Not measured; expected to be much slower than the per-pair models and to break SIZE-05/PERF-01.
  - Also possible, not measured: Hugging Face `candle` (pure Rust; has Marian, no M2M-100 in its model list) and ONNX Runtime via `ort`.
  - Engines sit behind `Translator`, so a switch touches only `pg-bergamot`, the manifest and `live.rs`.

---

## M0 — Spikes and foundations

### Repository and tooling
- [x] M0-REPO-1 Cargo workspace with `crates/pg-{core,capture,ocr,translate,pipeline,models,bench}`.
- [x] M0-REPO-2 `.gitignore`, `rustfmt.toml`, workspace lints, `deny.toml`.
- [~] M0-REPO-3 CI skeleton `.github/workflows/ci.yml`: fmt, clippy `-D warnings`, tests on Windows/macOS (arm64 + x86_64)/Ubuntu, `pg-bench` smoke run, `cargo deny` (QUAL-01, SEC-05 skeleton). *Written but never run: no remote yet. `cargo deny check` passes locally (cargo-deny 0.20.2, two documented advisory ignores, D-6). CI has no MSRV (1.90) job yet.*
- [x] M0-REPO-4 Docs skeleton: README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, GOVERNANCE, PRIVACY, docs/ARCHITECTURE.md, docs/adr/0000-template + ADR-0001 (OSS-03 skeleton). *Contact e-mails are `.invalid` placeholders.*
- [ ] M0-REPO-5 Add LICENSE-MIT / LICENSE-APACHE once D-2 is decided.
- [~] M0-REPO-6 Repo history + public remote. *Done: local repo + initial commit `8cd14de`. Not done: remote, real contact addresses, `.gitattributes` (global `core.autocrlf` converts files to CRLF on Windows checkouts).*
- [ ] M0-REPO-7 `cargo-about` config for THIRD_PARTY_NOTICES (OSS-01).

### Headless core (Principle 4)
These are verified with mocks. Each requirement is fully met only once real engines and the app use these crates.
- [x] M0-CORE-1 `pg-core`: PhysRect/LogicalRect + scale conversion (CAP-04), Frame (Debug never prints pixels), Line, Block (soft-hyphen joining), LangTag, LangPair, SourceLang, Error. *18 tests.*
- [x] M0-CORE-2 `pg-capture`: `ScreenCapturer` trait, `StaticCapturer`, `FixtureCapturer` (PNG). *FixtureCapturer is untested because there are no images yet.*
- [x] M0-CORE-3 `pg-ocr`: `OcrEngine` trait, `MockOcr`, `default_engine` (OCR-01 table), `resolve_language` (OCR-06, SV-02). *4 tests.*
- [x] M0-CORE-4 `pg-translate`: `Translator` trait, `TranslateError` + Fluent IDs + HTTP mapping (TR-06), `MockTranslator`, `PivotTranslator`/`plan_route` (TR-03), `langid` (SV-04, TR-07), `tokens::check_preserved` (SV-05). *18 tests incl. `sv04_mixed_fixture_accuracy_*`, `sv05_numbers_fixture_pairs`.*
- [x] M0-CORE-5 `pg-pipeline`: `change` (CAP-03, CAP-06), `grouping` (OCR-02 + reading order), `cache` (TR-04, NFC keys, 2,000 entries), `scheduler` (150 ms rest, 4 Hz recheck, generation, parking), `color` (REN-01 ≥ 4.5:1), `Pipeline::run_cycle` (TR-05 batching, TR-07 passthrough, OCR-05 hide/dim, SV-02 Auto fallback), `driver::spawn`, `LensUpdate` (serde, text-free Debug). *25 unit + 19 integration tests (`tests/pipeline.rs`), incl. `cap03_stale_results_are_never_drawn`, `lens06_hidden_lens_runs_no_cycles`.*
- [x] M0-CORE-6 `pg-models`: `Manifest` (MDL-02 validation, MDL-01 Swedish-first catalog, SV-01 wizard models), `ModelStore` (MDL-05 delete/usage, MDL-04 import), `Downloader` (MDL-03 resume via Range, cancel, progress; https-only except loopback). *7 unit + 5 tests against a local HTTP server.*
- [x] M0-CORE-7 `pg-bench`: `--fixture/--target/--cache/--runs/--warmup/--scale/--json`, nearest-rank p50/p95/p99 per stage. Release build on this machine with the mock engines: total p50 0.47 ms; change detection 0.32 ms, within the 2 ms budget.

### Spikes (each ends with an ADR in `docs/adr/`)
- [~] M0-SPK-1 Transparent, frameless, always-on-top Tauri 2 window; click-through via cursor hit-testing at ~30 Hz (PLAN §5). Measure smoothness on Win/mac/Linux. → ADR-0002. *Windows done and measured (19–49 ms switch, ≈0.3% core while visible, 0 when hidden; debug build). macOS/Linux not run; release-build numbers not taken.*
- [~] M0-SPK-2 Self-excluding region capture: WGC + `WDA_EXCLUDEFROMCAPTURE` (Win), ScreenCaptureKit (mac); X11 hide/capture/show flicker timing. → ADR-0003. *Windows done and measured (ADR-0003). macOS ScreenCaptureKit and X11 flicker timing not started.*
- [~] M0-SPK-3 Bergamot (cxx FFI) vs CTranslate2 (`ct2rs`) with sv↔en models: build complexity, size, latency, memory on all OSes. → ADR-0004. *Windows done (ADR-0004, accepted for Windows): CTranslate2 (`ct2rs` 0.10.3, ruy) + OPUS-MT int8 measured, plus a third option, `fxtranslate` (pure-Rust Bergamot port) + Mozilla's models, which was chosen. Not done: Bergamot C++ via `cxx` wasn't built (running CMake on the downloaded source wasn't permitted in session 4; facts from its CMake files only), and nothing was built or timed on macOS/Linux/arm64.*
- [ ] M0-SPK-4 Swedish fixtures + OCR accuracy/latency per engine incl. Vision vs Tesseract `swe` on macOS. → ADR-0005.
- [ ] M0-SPK-5 Wayland portal screenshot + fullscreen Freeze window on GNOME and KDE. → ADR-0006.

### Fixtures (SPEC §5, CC0)
- [~] M0-FIX-1 Ground-truth text for `ui-dialog-sv`, `ui-dialog-en`, `mixed-sv-en`, `numbers-sv`, `numbers-en`, `paragraphs-sv`, `paragraphs-en`, `long-compounds-en` in `tests/fixtures/*/truth.json` (format in `tests/fixtures/README.md`). *Done: text, block lists, expected languages. Not done: `bbox` per block, images, a held-out `mixed-sv-en-2` for langid, and a native-speaker review of the Swedish.*
- [x] M0-FIX-2 Fixture image generator (render truth.json to PNG at 100/125/150/200%, light + dark theme, 12–16 px UI fonts). *`pg_testkit::fixture::FixtureImage::render` (Rust, `ab_glyph` + the OS UI font): 600 logical px page, block i in 12–16 px (`i % 5`), word-wrapped, light `#F3F3F3`/`#1B1B1B` and dark `#202020`/white; returns each line's text and box. **Rendered at test time, not committed**: the font (Segoe UI here) isn't redistributable and CC0 for its renderings is unclear (D-7); each OS is tested on its own UI font; no images to keep in sync. Deterministic per font file (`rendering_is_deterministic`; the font's SHA-256 is recorded). `cargo run -p pg-testkit --bin render-fixtures` writes all 64 PNGs + line boxes (≈ 3 MB) to `target/fixture-images`. Wired in: `FixtureCapturer::from_png`, `pg-bench` (`--scale`, `--theme`), CAP-04 tests, OCR gates. Tests: `fixture::tests` (5).*
- [x] M0-FIX-3 CER scorer (per-char + å/ä/ö-specific error report, SV-03) in `pg-bench`. *Library: `pg_testkit::metrics::score` (NFC + whitespace-normalized CER and WER; Levenshtein alignment reporting each misread å/ä/ö/Å/Ä/Ö as a substitution or deletion; scores add up over sets; `systematic(n)`). `pg-bench --ocr windows` prints `accuracy  CER … WER … åäö … [ä→a×1]` (JSON: `accuracy`) for the whole lines in the lens. Tests: `metrics::tests` (6).*
- [ ] M0-FIX-4 Eval sets `tests/eval/sv-en`, `tests/eval/en-sv`, 200 segments each with references (SV-07).

---

## M1 — MVP on Windows + macOS

### Capture
- [x] M1-CAP-1 Windows: Windows.Graphics.Capture region capture (CAP-01) in `pg-capture::windows`. *`pg_capture::WgcCapturer` (module `wgc`, the only `unsafe` in the crates).*
  - *Per-monitor sessions keep only the newest frame; the region is cropped on the GPU; monitor rects are read in a PMv2 thread scope so they are physical pixels; parts outside every monitor are black.*
  - *`ScreenCapturer::release` frees the sessions while the lens is hidden or paused. The driver calls it, and `lens06_hide_parks_the_pipeline` checks that.*
  - *Tests: `cap01_captures_exactly_the_region`, `cap04_every_monitor_and_a_region_across_two` (100% + 150% at negative coordinates, region across x = 0), `release_closes_sessions_and_capture_reopens_them`.*
  - *600×300 capture p50 0.26 ms / p95 0.41 ms (release); the first capture after a session starts takes 10–37 ms.*
  - *Not tested: hot-plug, resolution change, device loss.*
- [x] M1-CAP-2 Windows: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on lens window; marker self-capture test over 100 cycles (CAP-02).
  - *`lens::exclude_from_capture` runs before the lens is first shown. If it fails, live mode doesn't start and the lens shows `cap-error-self-exclusion`.*
  - *Tests: `cap02_excluded_window_is_not_captured` and `cap02_marker_is_never_recognized_over_100_cycles`. The second uses a Win32 stand-in window that draws "POLYGLASS MARKER QX7" with the same affinity call; a control run without exclusion does read the marker.*
  - *Manually: in live mode the lens shows one `[en] …` translation of the text beneath it, never a translation of its own output.*
  - *Gap: the marker is not drawn by the real lens.*
- [ ] M1-CAP-3 macOS: ScreenCaptureKit stream, `sourceRect` = lens, excluding own app (CAP-01/02).
- [ ] M1-CAP-4 macOS Screen Recording permission missing/revoked → explanation + open-settings button + auto-resume (CAP-05).
- [x] M1-CAP-5 HiDPI/mixed-DPI fixture tests at 100/125/150/200% (CAP-04, LENS-02). *Real-capture tests at 100% and 150% and across a mixed-DPI monitor edge (`cap04_every_monitor_and_a_region_across_two`); the live lens was checked on both monitors (ADR-0003). Fixture tests at all four scales (`crates/pg-capture/tests/fixture.rs`): `cap04_lens_over_a_line_at_every_scale` (lens in logical px over each separated line of `ui-dialog-en`, light + dark, `LogicalRect::to_physical`; the capture holds exactly that line's ink) and `cap04_windows_ocr_reads_the_lens_at_every_scale` (100 px lens moved down the page, Windows OCR CER ≤ 5%: 3.6/3.3% at 100%, 0–1.8% above). Not tested: real monitors at 125% and 200% (none here).*

### OCR
- [x] M1-OCR-1 Windows OCR engine via `windows` crate (`Windows.Media.Ocr`), lines + boxes + confidence (OCR-01/02). *`pg_ocr::WindowsOcr` (windows 0.62 since session 5, the version Tauri 2.12 uses; no `unsafe`). Line box = union of word boxes in frame pixels; confidence is always 1.0 because Windows OCR has none, so OCR-05 filtering does nothing with this engine. Tests in `crates/pg-ocr/tests/windows_ocr.rs` render text with `pg-testkit` (Segoe UI, grayscale AA): `ocr01_recognizes_rendered_english_lines_with_boxes`, `ocr01_dark_theme_and_pixel_formats` (BGRA/RGBA with padded stride/Gray8), `ocr01_boxes_are_in_frame_pixels_at_any_upscale`. Text is checked with a CER bound (≤ 6%/line), not exact match. PERF-03 (release, `pg-bench --ocr windows`, 600×300, sv): p50 18.9 ms / p95 19.1 ms at 100%; 34.0 / 35.9 ms at 150% (900×450 px).*
- [~] M1-OCR-2 Runtime language query `AvailableRecognizerLanguages` (OCR-06) + SV-02 Windows message/link when `sv-SE` missing. *Done: `WindowsOcr::available_languages()` is queried on every `recognize` that names a language and by `supported_languages()`; a missing language is `Error::OcrLanguageMissing`. Tests: `ocr06_languages_are_queried_at_runtime`, `ocr03_uninstalled_language_is_reported_missing` (real engine, `tlh`), `sv02_swedish_missing_or_recognized` (checks whichever branch the machine is on). **sv-SE is now installed on this machine**, so the real "Swedish missing" branch can't be hit here any more. Lens UI: when Swedish OCR is missing (the Auto-mode notice or `ocr-error-language-missing`), the interior explains where to add it and a toolbar "⚠ SV" button calls `open_ocr_language_settings`, which opens `ms-settings:regionlanguage`. The button is in the toolbar because the interior is click-through. Not verified in the running app: Swedish is installed here, so that state never occurs.*
- [ ] M1-OCR-3 Apple Vision engine (English) + runtime language check (OCR-01, OCR-06).
- [ ] M1-OCR-4 Tesseract 5 engine (bundled on macOS with `swe`+`eng`) (OCR-01, SV-02 mac).
- [x] M1-OCR-5 Upscale small text before OCR (PLAN §10 risk mitigation). *`WindowsOcr` enlarges frames 2× (bilinear) below 200% scale; `with_upscale(k)` forces a factor. Measured on rendered Segoe UI, 5 lines (3 en, 2 sv), 600×300, release: exact lines at 1×/2×/3× — 11 px 0/0/1, 12 px 0/2/2, 13 px 1/2/2, 14 px 2/3/2, 16 px 2/3/2, 20 px 5/5/5; at 1× small text drops whole words. OCR time 1× ≈ 8–17 ms, 2× ≈ 17–26 ms, 3× ≈ 30–35 ms. Remaining misreads at 2× are mostly 5→S and rn→m (`ändringama`). Tuned on synthetic text only; recheck on real captures (ClearType).*
- [~] M1-OCR-6 Accuracy gates on fixtures: OCR-04 (en), SV-03 (sv, å/ä/ö). *`crates/pg-ocr/tests/accuracy.rs`: each fixture at 100–200%, light + dark, read by Windows OCR (default upscaling) in lens-sized strips of whole lines (600×300 logical), scores summed per set. English set = `*-en` (4), Swedish set = `*-sv` + `mixed-sv-en` (sv recognizer). Skips with a message without the font or OCR language (`POLYGLASS_REQUIRE_OCR_GATES=1` fails instead).*
  - *`ocr04_english_fixture_set_cer`: **passes**, CER 0.80% (81/10,160), gate ≤ 3%.*
  - *`sv03_swedish_fixture_set_cer`: **passes**, CER 0.82% (144/17,504), gate ≤ 3%. Also holds å/ä/ö ≤ 10% as a regression guard (not the SPEC bound).*
  - *`sv03_swedish_letters` (`#[ignore]`d): **fails**, å/ä/ö 7.3% (35/480), gate ≤ 2%, with repeated misreads ä→å ×16, ä→a ×8, ä→i ×4, ä→∅ ×3, ä→u ×2, ö→o ×2. All errors are at 100% (13 + 11) and 125% (5 + 6); 150% and 200% have none. Forcing 1×/3×/4× upscaling doesn't help (6.7% / 11.9% / 9.8%). Next: Next up 1.*
  - *Measured CER % per image (release build, Windows 11, Segoe UI 5.72; L = light, D = dark; in brackets: misread å/ä/ö of total):*

    | Fixture | 100% L | 100% D | 125% L | 125% D | 150% L | 150% D | 200% L | 200% D |
    |---|---:|---:|---:|---:|---:|---:|---:|---:|
    | `long-compounds-en` | 0.48 | 1.43 | 0 | 0 | 0 | 0 | 0 | 0 |
    | `numbers-en` | 2.40 | 2.99 | 0.30 | 1.50 | 1.20 | 1.20 | 0 | 0 |
    | `paragraphs-en` | 3.34 | 1.11 | 0 | 0 | 0 | 0 | 0 | 0 |
    | `ui-dialog-en` | 3.61 | 3.25 | 0 | 0.36 | 0 | 0 | 1.81 | 0 |
    | `mixed-sv-en` | 1.88 [7/23] | 2.51 [6/23] | 0.54 [4/23] | 0.54 [4/23] | 0.09 | 0.09 | 0 | 0 |
    | `numbers-sv` | 2.40 | 2.10 | 0.60 | 0 | 0 | 0.30 | 0 | 0 |
    | `paragraphs-sv` | 2.09 [4/23] | 6.28 [4/23] | 0.42 | 0.63 [1/23] | 0 | 0 | 0 | 0 |
    | `ui-dialog-sv` | 1.15 [2/7] | 4.23 [1/7] | 0.77 [1/7] | 0.77 [1/7] | 0 | 0 | 0 | 0 |

    *Per slice (English / Swedish CER): 100% L 2.68 / 1.92, 100% D 2.13 / 3.47, 125% L 0.08 / 0.55, 125% D 0.47 / 0.50, 150% L 0.31 / 0.05, 150% D 0.31 / 0.09, 200% L 0.39 / 0, 200% D 0 / 0. The numbers are identical across runs.*

### Translation and models
- [x] M1-TR-1 On-device backend per ADR-0004 implementing `Translator`, `is_local() == true` (TR-01). *New crate `pg-bergamot`: `BergamotTranslator` (id `bergamot`), translation on Tokio's blocking pool. Tests: `without_models_every_pair_is_model_missing`, `a_broken_installed_model_is_an_error_not_a_panic`; real models: `tr1_tr3_both_directions_loaded_and_translate` (ignored, run manually: passes). The app uses it in live mode. Manually verified with `lens-read.ps1` (release build): "Vill du spara ändringarna innan du stänger?" → "Do you want to save the changes before you close?", English passed through.*
- [x] M1-TR-2 Model manifest entries for sv→en, en→sv with real URLs/SHA-256/licenses (MDL-02, SIZE-05). *`bergamot-sv-en`, `bergamot-en-sv`: Mozilla GCS https URLs, the gzip files' sizes + SHA-256, MPL-2.0, 30.5 MB together. Tests: `bundled_manifest_has_both_swedish_directions` (incl. < 100 MB); `mdl02_tr2_swedish_models_download_and_verify` (ignored: downloads through `pg-models` and checks the unpacked models against Mozilla's registry hashes; passes). Also downloaded through the app's toolbar ⬇ (2.5 s here).*
- [x] M1-TR-3 Both directions loaded → instant swap (SV-01). *`load_installed` loads every installed direction at startup (≈ 0.4 s for both in the app, off the UI thread). `tr1_tr3_both_directions_loaded_and_translate` alternates sv→en / en→sv without reloading (each pair of short strings well under 500 ms). The UI swap button and hotkey are still placeholders (M1-APP-9).*
- [x] M1-TR-4 Preserved-token gate on `numbers-sv`/`numbers-en` with the real backend (SV-05). *`pg_translate::TokenGuard` (masked `{n}` retry, else the source text) + `tr4_sv05_numbers_fixtures_keep_preserved_tokens` (ignored, run manually): all 20 blocks pass `check_preserved`, 9 needed the masked pass, 0 fell back to the source. The guard logic is also unit-tested with a mock (`guard::tests`, 8 tests, part of `cargo test --workspace`).*

### Desktop app (`apps/desktop`)
- [x] M1-APP-1 Tauri 2 scaffold, TS + Vite UI, strict CSP, least-privilege capabilities (SEC-01, SEC-02). *Verified on Windows: the app runs in `tauri dev` and with embedded assets (`--features tauri/custom-protocol`) under the production CSP. The commands are declared in a `build.rs` app manifest, so each needs `allow-*` in `capabilities/lens.json`; no `core:default`. Validation tests: `sec01_nudge_accepts_only_arrow_key_steps`. CI `ui` job and Linux webkit deps added but never run; `tauri build` (bundling) not run yet.*
- [~] M1-APP-2 Lens window: launch/center/restore (LENS-01), drag + arrow keys (LENS-02), resize limits (LENS-03), click-through (LENS-04). *Manually verified on Windows with `apps/desktop/scripts/lens-probe.ps1`:*
  - *First launch is centred on the cursor's monitor at 480×200 and stays on top.*
  - *The lens has no taskbar button.*
  - *Toolbar drag, arrow keys (1 px) and Shift+arrow (10 px) work.*
  - *Corner and edge resize work, and the 120×48 minimum holds.*
  - *Click-through works over the interior only.*
  - *Dragging onto the 150% monitor gives the correct 720×300 physical size and a fresh pipeline result.*

  *Not done:*
  - *Restore of last size/position/mode (needs SET-01).*
  - *The max size (monitor work area) is set only at launch, not when the lens moves to another monitor.*
  - *The click-through hotkey.*
  - *The toolbar's click-through toggle is implemented, but its effect was not checked with input.*
- [~] M1-APP-3 Toolbar with accessible controls + privacy indicator (LENS-05, PRIV-05). *Stub done:*
  - *Every control is a `<button>` with an accessible name.*
  - *The privacy indicator shows a lock or a cloud with the provider name, driven by `LensUpdate.backend.local`.*
  - *Pause/resume parks the pipeline (verified: lens cleared, CPU drops).*
  - *Close hides the lens to the tray.*

  *Not done:*
  - *A mode badge: "DEMO" with `--demo`, "MOCK MT" in live mode until M1-TR-1.*
  - *Source/target/swap/mode/settings are `aria-disabled` placeholders.*
  - *Keyboard Tab order and screen-reader output not tested.*
- [~] M1-APP-4 Tray icon, no taskbar/Dock entry, show/hide hotkey parks the pipeline (LENS-01, LENS-06). *Done:*
  - *Tray icon with Show/hide and Quit, plus left-click toggle.*
  - *No taskbar entry (verified).*
  - *Hiding (toolbar close or Alt+F4) parks both the pipeline and the click-through poller (verified 0 ms CPU over 10 s).*

  *Not done:*
  - *The tray menu wasn't clicked in testing.*
  - *The global hotkey.*
- [~] M1-APP-5 Wire `pg-pipeline` driver to lens events; render `LensUpdate` in Overlay mode (MODE-01, REN-01). *Done:*
  - *Move/resize/scale/show/hide/pause events go through `state::Lens` into the `DriverHandle`.*
  - *The interior region is computed with `LogicalRect::to_physical`.*
  - *`lens-update` events are drawn as positioned DOM blocks with the sampled bg/fg (`#rrggbb` only) and `textContent` only.*
  - *A basic 16→9 px shrink-to-fit (full REN-02 is M1-APP-6).*
  - *Verified end to end with `--demo` (mock engines). Test: `demo_pipeline_translates_swedish_and_passes_english_through`.*

  *Since session 3 the default (non-demo) pipeline on Windows is real capture + Windows OCR (`live.rs`), checked with `apps/desktop/scripts/lens-read.ps1` on the 100% and 150% monitors. Not done: real translation (M1-TR-1).*
- [ ] M1-APP-6 Font fitting down to 9 px + grow + tooltip (REN-02).
- [ ] M1-APP-7 Hold Alt/Option to show original (REN-04); copy hotkey (REN-05, UTF-8 round-trip test).
- [ ] M1-APP-8 Settings TOML in config dir (SET-01); model manager UI (MDL-01/03/05); first-run wizard (MDL-06, SV-01).
- [ ] M1-APP-9 Default hotkeys (HK-01) + Swedish-layout AltGr check (SV-09).
- [ ] M1-APP-10 `--trace-timings`, `--debug-dump-frames` with red banner (PRIV-02); default logs contain no text (PRIV-07).
- [ ] M1-APP-11 Local crash reports, no telemetry (PRIV-06).

### Verification
- [ ] M1-VER-1 Offline test (PRIV-01) on Win + mac.
- [ ] M1-VER-2 PERF-01/02/05/06 on REF-A/REF-B with real engines.

---

## M2 — Linux + more modes
- [ ] M2-1 X11 capture (x11rb + MIT-SHM); experimental overlay hide-capture-show (CAP-02 X11).
- [ ] M2-2 Panel mode (MODE-02) incl. flip-above and reading order; Panel is the default on Linux X11 (D-5).
- [ ] M2-3 Freeze mode on all platforms; Wayland via xdg-desktop-portal (MODE-03, CAP-05 Wayland).
- [ ] M2-4 Tray-menu fallback for hotkeys on Wayland (HK-03).
- [ ] M2-5 Low-confidence hide/dim setting (OCR-05) — pipeline side already filters by threshold; UI setting pending.
- [ ] M2-6 "No text found" for blank regions in UI (CAP-06) — detection done in pipeline.
- [ ] M2-7 Linux packaging deps on Swedish Tesseract data; AppImage bundles it (SV-02 Linux, SIZE-03/04).

## M3 — Online backends
- [ ] M3-1 LibreTranslate backend + language-list check for Swedish (TR-01, SV-10).
- [ ] M3-2 DeepL Free/Pro + EN-GB/EN-US variant, default EN-US (TR-01, SV-10, D-5).
- [ ] M3-3 Google Cloud Translation (TR-01).
- [ ] M3-4 Batching per frame (TR-05); 5 s timeouts and error mapping (TR-06) — `TranslateError` kinds already exist. Online failure falls back to on-device when the pair's models are installed, with a lens notice; on by default, can be turned off (TR-06, D-5).
- [ ] M3-5 Keychain storage via `keyring` (SET-02) + grep test.
- [ ] M3-6 Consent dialogs per backend (PRIV-04); text-only payloads test (PRIV-03).
- [ ] M3-7 Network activity log (PRIV-08); rustls only (SEC-03).
- [ ] M3-8 Configurable language pairs (MDL-07, D-5): pairs come from settings + the model catalog instead of the hard-coded sv/en (`SourceLang::AutoSvEn`, `langid` sv/en scorer, `live.rs`, `pg-bergamot` loading exactly sv→en/en→sv); toolbar source/target pickers; OCR language follows the source (OCR-06). Could move earlier if it simplifies M3-9.
- [ ] M3-9 German → English on-device (SPEC §3, D-5): manifest entry for Mozilla's de→en model (verify it's in the catalog, its license and SHA-256), German OCR (Windows `de-DE` OCR feature, Tesseract `deu` elsewhere), `de-en` fixtures with ä/ö/ü/ß, CER gate.

## M4 — Polish
- [ ] M4-1 Appearance settings + reduced motion (LENS-07).
- [ ] M4-2 Rebindable hotkeys with conflict/AltGr reporting (HK-02).
- [ ] M4-3 Screen-reader live region with `lang` (REN-06).
- [ ] M4-4 `lang="sv"` hyphenation / break-at-edge (SV-06).
- [ ] M4-5 Fluent string externalization; Swedish UI + Swedish PRIVACY.md (I18N-01, SV-08).
- [ ] M4-6 A11Y pass (A11Y-01/02).
- [ ] M4-7 Offline model import (MDL-04) — verify path already in `pg-models`.

## M5 — 1.0 hardening
- [ ] M5-1 Signing + notarization (OSS-04), signed opt-in updater (SEC-04).
- [ ] M5-2 Release pipeline: signed tags, SHA256SUMS, CycloneDX SBOM, provenance (OSS-02).
- [ ] M5-3 `pg-bench` regression gate (>20% p50) in CI (QUAL-02); coverage ≥70% (QUAL-03).
- [ ] M5-4 Packaging: MSI/NSIS, DMG, .deb, .rpm, AppImage, Homebrew, winget; size gates (SIZE-*).
- [ ] M5-5 RES-01..05 measurements.
- [ ] M5-6 SV-07 chrF report + native-speaker rating.

---

## Session log

Newest first. Keep each entry short: what changed, what's verified, what's next, gotchas.

### 2026-11-01 — Session 7 (owner decisions D-1..D-5, coordinator)
- The owner answered D-1..D-5. D-1, D-2, D-3 and D-5 are decided and D-4 is deferred (details under "Owner decisions"). SPEC is now v0.3: §3, §4, MODE-02, TR-06, MDL-07 (new), SV-01, SV-10, §8 and §10. PLAN §8 and §11 are updated too.
- D-2 is done: `LICENSE` (Apache-2.0; matches the text that `reqwest` ships, apart from the copyright line), workspace `license`, `package.json` + lock and the README.
- New tasks: M3-8 (configurable language pairs, MDL-07) and M3-9 (German → English). M2-2, M3-2 and M3-4 carry the D-5 details.
- New open question D-8 (`fxtranslate` vs Bergamot C++), raised by the owner.
- README: "Build and run locally" section (`5c50fc7`). The release build (`npm run tauri build -- --no-bundle`) and a `--demo` launch of `target/release/polyglass.exe` were checked; the installer bundle (WiX/NSIS) was not.
- Next: see "Next up"; D-8 may move the engine switch to the front.

### 2026-10-31 — Session 6 (M0-FIX-2, M0-FIX-3, M1-CAP-5, M1-OCR-6)
- Commits: `d182124` CER/WER/å-ä-ö scoring (`pg_testkit::metrics`); `f190a2a` fixture image generator (`pg_testkit::fixture`, `render-fixtures`); `e6df08d` `FixtureCapturer::from_png` + CAP-04 fixture tests at 100–200%; `1fbcbb0` `pg-bench` on fixture images with OCR accuracy output; `f63c542` OCR-04/SV-03 gates; then the docs and this checklist.
- Fixture images are rendered at test time and not committed (font licensing, D-7 added; details in `tests/fixtures/README.md`).
- Verified: fmt, clippy `-D warnings`, 151 tests (5 ignored), `cargo +1.90 check --workspace --all-targets`, `cargo deny check` (all ok). `pg-bench --ocr windows` (release, `ui-dialog-sv@100-light`): OCR p50 16.9 ms, accuracy CER 1.15%.
- Result: OCR-04 passes (English CER 0.80%) and SV-03's CER part passes (0.82%), but SV-03's å/ä/ö part fails (7.3%, mostly ä→å/ä→a at 100–125%). M1-OCR-6 stays `[~]`; the failing gate is an `#[ignore]`d test.
- Not verified: whether the å/ä/ö failure also happens on real ClearType screen text; real 125%/200% monitors; macOS/Linux fonts (Arial, DejaVu Sans) in the generator.
- Gotchas:
  - Windows OCR misses words near the frame edge: give OCR tests some background around the text, or they measure the crop instead of the engine.
  - The gates read each page in lens-sized strips (`FixtureImage::bands`), as the lens would see it; whole pages fit Windows OCR's limit (`MaxImageDimension` is 10,000 px here).
  - Commits here were split by staging intermediate file versions with `git hash-object -w` + `git update-index --cacheinfo` (no interactive `git add -p` in this environment).
- Next: see "Next up".

### 2026-10-31 — Session 5 (tooling: MSRV 1.90, Tauri 2.12, cargo deny)
- MSRV fix: `pg-capture` is declared with `default-features = false` in `[workspace.dependencies]`; `pg-pipeline` and the app inherit it plainly, `pg-bench` adds `fixture`. `cargo tree` shows the same `pg-capture` features per package as before.
- `deny.toml`: `allow-wildcard-paths = true` under `[bans]`.
- Verified: `cargo +1.85 check --workspace --all-targets`, clippy `-D warnings`, 137 tests (4 ignored); `cargo deny check bans licenses sources` ok. `advisories` still fails: `cargo update` stays put because every fix needs Rust ≥ 1.88 (MSRV-aware resolver). Listed as D-6, no ignores added.
- Then the owner decided D-6 ("Raise MSRV to 1.90"): `rust-version = "1.90"`; Tauri crate 2.12.5 (`tauri = "2.12"`, `tauri-build = "2.7"`) and npm `@tauri-apps/api` / CLI 2.12.4 (no plugins); `cargo update` (77 packages: plist 1.10.3 without quick-xml, time 0.3.55, urlpattern 0.6, no `unic-*`); `windows` 0.61 → 0.62 to match Tauri 2.12 (`IAsyncOperation::get` → `join`); two `[advisories] ignore` entries with reasons. Let-chains and `as_chunks_mut` replace the MSRV workarounds that clippy now flags.
- Verified: `cargo +1.90 check --workspace --all-targets`; `cargo fmt --check`; clippy `-D warnings` (stable and 1.90); 137 tests (4 ignored); `cargo deny check` passes in full; `npm run build`. `npm run tauri dev`: live mode starts, both models load, the first cycle translated on-screen text (capture 38 ms, OCR 45 ms, translation 632 ms cold, then cache hits), click-through on in the interior (`WS_EX_TRANSPARENT` set) and off on the frame. Not verified: macOS/Linux, a release build on 2.12.
- Gotcha: from Git Bash, `npm run tauri dev` fails with "cargo metadata … program not found" (rustup shim); put `$(rustc --print sysroot)/bin` first on `PATH`.
- Next: add a `cargo +1.90 check` job to CI.

### 2026-10-31 — Session 4 (M0-SPK-3 Windows, M1-TR-1..4)
- ADR-0004: compared CTranslate2 (`ct2rs` 0.10.3 + OPUS-MT int8) with `fxtranslate` 0.5 (pure-Rust port of the Bergamot engine) + Mozilla's Firefox Translations models, measured on this machine. Picked the Mozilla models on `fxtranslate`: 30.5 MB for both directions vs ≈ 162 MB, MPL-2.0 vs CC-BY-4.0, no CMake and no `unsafe`, 7 ms short / 95 ms paragraph (scalar, 1 thread) vs 27 / 199 ms (ruy, 4 threads), 68 MB vs 200 MB RSS for both directions. Bergamot C++ via `cxx` was **not built** (see Gotchas).
- New: `pg_translate::guard` (`TokenGuard`, SV-05), crate `pg-bergamot`, manifest entries, app live mode on the real backend with a toolbar ⬇ download (`models.rs`, command `download_models`, event `lens-models`). The "MOCK MT" badge now shows only if the mock backend is active.
- Verified: fmt, clippy `-D warnings`, 137 tests (11 new; plus 3 ignored real-model tests, run manually and passing). In the release app (`--features tauri/custom-protocol`): model-missing message + ⬇ → download → real translation under the lens; a restart loads both models from disk; logs contain no screen text; polyglass.exe working set 126 MB with both models.
- Not verified: macOS/Linux; PERF-01 end to end via `pg-bench`; translation quality (SV-07).
- `cargo deny`: installed and run for the first time; licenses pass, bans/advisories fail on pre-existing items only (Known follow-ups).
- Gotchas:
  - **Running CMake on the cloned `mozilla/translations` source was blocked by this environment's permission classifier** ("code from external"). Building crates.io packages (ct2rs compiles CTranslate2 with CMake) was allowed. Finishing option A needs the owner to allow that build.
  - ct2rs on Windows: run inside `vcvars64` with `CMAKE_GENERATOR=Ninja` (VS 18 ships CMake + Ninja under `Common7\IDE\CommonExtensions\Microsoft\CMake`). A long `CARGO_TARGET_DIR` breaks sentencepiece's try-compile (250-char object path), and a CMake cache from another generator must be deleted. `cmd /c` children can't start `~/.cargo/bin/cargo.exe` (rustup symlink); put the toolchain's `bin` first on `PATH`.
  - `ct2rs` `dnnl`: oneDNN builds but won't link with MSVC (CRT `/MD` vs `/MT`, either way round).
  - fxtranslate: `translate_batch` is slower than one block at a time; `translate` truncates multi-sentence input, so use `translate_long`. Its gemmology shim fails on MSVC and the build silently falls back to scalar (`FXTRANSLATE_REQUIRE_SIMD=1` makes that an error).
  - The Mozilla model files are gzip. The manifest pins the `.gz` hashes and the files stay gzipped on disk (`pg-bergamot` decompresses on load). `{1}`-style placeholders survive the model; `X1X` does not reliably (`kl. X2X`).
  - The Rust 1.85 toolchain is now installed (`cargo +1.85`), but the workspace doesn't load with it (Known follow-ups).
- Next: see "Next up".

### 2026-10-31 — Session 3 (M1-OCR-1/2/5, M1-CAP-1/2)
- Commits: `d061cf8` Windows OCR + `pg-testkit`; `7257279` WGC capturer + `ScreenCapturer::release`; `3640236` live mode in the app; then ADR-0003 + this checklist.
- `pg_ocr::WindowsOcr` (no `unsafe`), with 2× upscaling below 200% scale and a runtime language list. PERF-03 on rendered text: OCR p50 18.9 ms at 600×300 (100%) and 34 ms at 150% (release).
- New crate `pg-testkit` (dev and bench only): `TextRenderer` draws text with a system font (Segoe UI). `pg-bench` renders the fixture text with it and has `--ocr windows`.
- `pg_capture::WgcCapturer` + ADR-0003. The app sets `WDA_EXCLUDEFROMCAPTURE` on the lens and runs live capture + Windows OCR + mock translation by default; `--demo` is unchanged.
- Verified: fmt, clippy, 126 tests (19 new). The live app was checked on both monitors with `lens-read.ps1`; logs had no recognized text; a hidden lens used 0 ms CPU per 10 s.
- Not verified: release-build CPU, the Windows 11 yellow capture border (check by eye), the SV-02 UI (Swedish is installed here), CI.
- Gotchas:
  - **Swedish OCR (`sv-SE`) is now installed on this machine**, so the real "Swedish missing" path can't be reached here. The engine's missing-language path is tested with `tlh` instead.
  - Windows OCR misreads small grayscale-AA text badly at 1× and drops whole words at 14 px and below. 2× bilinear fixes most of it; 3× doesn't help. Typical leftovers: `5`→`S`, `rn`→`m`. Tests use a CER bound, not exact strings.
  - The lens is invisible to screenshots (that's CAP-02 working). Read its text through UI Automation with `apps/desktop/scripts/lens-read.ps1 -X <x> -Y <y>`: put the test window under the lens interior; `lens-probe.ps1 info` gives the lens rect.
  - Windows PowerShell 5.1 reads BOM-less UTF-8 scripts as ANSI (å/ä/ö become mojibake). Save `.ps1` files that contain non-ASCII text with a BOM.
  - In this environment's Bash tool, a doubled backslash inside a quoted heredoc reaches the program as a single one, so Python read `\b` as a backspace. That's how README's PATH hint got a literal backspace (fixed). Heredocs with apostrophes also sometimes fail to parse; write scripts to a file instead, and check for control characters after scripted edits.
  - windows-rs 0.61: the D3D11 and WGC classes are `Send`, but the WinRT `IDirect3DDevice` isn't, so `WgcCapturer` creates it whenever a session starts instead of storing it. `HMONITOR` holds a raw pointer (not `Send`) and is stored as `isize`.
  - Let-chains (`if let … && …`) need Rust 1.88; the MSRV is 1.85.
  - A WGC frame pool fills up if nobody takes frames, and newer frames are then dropped. The `FrameArrived` handler keeps only the newest frame, and the copy holds that frame's lock so its buffer can't be recycled mid-copy.
  - The WGC tests take about 7 s and flash small topmost windows near (200, 200) on each monitor; they run one at a time behind a mutex.
- Next: see "Next up".

### 2026-10-31 — Session 2 (M0-SPK-1 + M1-APP-1)
- Added `apps/desktop`: Tauri 2 shell (`src-tauri`, now a workspace member) and the lens UI (`ui/`, TS + Vite 8, no framework), with a strict CSP and least-privilege capabilities. Modules: lens geometry, click-through poller, shared state → `DriverHandle`, IPC commands, `--demo` mock pipeline, tray.
- ADR-0002 (click-through by ~30 Hz cursor polling) accepted for Windows, with measurements. `apps/desktop/scripts/lens-probe.ps1` is the Win32 probe used for the manual checks.
- CI: Linux webkit/appindicator packages for the workspace build, and a `ui` job (npm ci/build/audit). Neither has run (no remote).
- Verified: fmt, clippy, 107 tests (9 new in `polyglass-desktop`). The app launches here in dev mode and with embedded assets under the production CSP. Manual window checks are listed under M1-APP-2..5.
- Not verified: macOS/Linux, release build/bundling, the tray menu by click, keyboard Tab order and screen readers.
- Gotchas:
  - **`npx tauri dev` fails with `cargo metadata ... program not found`** on this machine. `~/.cargo/bin/cargo.exe` is a zero-byte symlink to `rustup.exe` that child processes started by Node/cmd can't start. Fix: prepend `C:\Users\Shash\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin` to `PATH` for that shell.
  - Tauri 2.12 needs Rust 1.90 > MSRV 1.85; `cargo update` silently stays on 2.11. The npm packages are pinned to 2.11.x to match (see Known follow-ups).
  - Plain `cargo build`/`test` compiles the app in dev mode (it loads `devUrl`, nothing embedded), so workspace checks don't need `npm run build` first. To test the production CSP without `tauri build`, use `npm run build` then `cargo build -p polyglass-desktop --features tauri/custom-protocol` and run `target/debug/polyglass.exe --demo`.
  - macOS transparency needs both the `macos-private-api` crate feature and `app.macOSPrivateApi: true`; tauri-build errors if they disagree. Both are set, but this was never compiled on a Mac.
  - On Windows, tao removes `WS_EX_LAYERED` along with `WS_EX_TRANSPARENT` when click-through turns off. Transparency still works (WebView2 composition).
  - Mock OCR lines must be more than 0.9 line-heights apart, or block grouping merges them into one mixed-language block.
  - PowerShell P/Invoke turns `$null` string arguments into `""`. Use `[NullString]::Value` (e.g. `FindWindow`).

### 2026-10-31 — Session 1
- Created this checklist. Scaffolded the workspace (edition 2024, resolver 3, workspace lints incl. `unsafe_code = "deny"`; platform crates will need a module-level `allow`), docs, ADR-0001, CI and `deny.toml`.
- Implemented M0-CORE-1..7. `cargo test --workspace`: 98 passed, and the timing-dependent driver tests passed 8 runs in a row (they use Tokio's paused clock). Clippy and fmt are clean.
- Fixture ground truth written for all 8 SPEC fixtures (text only).
- Not done or not verified: CI never ran (no remote), `cargo deny` never ran, nothing tested on macOS/Linux.
- Committed as `8cd14de` on `main`, with no DCO `Signed-off-by` line (CONTRIBUTING asks contributors for one).
- Gotchas:
  - Windows OCR here has no `sv-SE`; Tesseract and Pillow aren't installed.
  - `tokio::select!` evaluates disabled branches' futures, so deadlines use `unwrap_or_else(Instant::now)`, not `unwrap()`.
  - `CycleOutcome::Update` holds a `Box<LensUpdate>` (clippy `large_enum_variant`).
  - SV-05 value checks compare only fractional/dotted numbers, because Swedish phone numbers (`08-123 456 78`) look like thousands grouping.
- Next: see "Next up" above.
