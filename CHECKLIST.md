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

- **Milestone:** M1. Headless core done. On Windows the lens runs **live by default**: Windows Graphics Capture of the lens interior + Windows OCR + **on-device translation** (`pg-ct2`: CTranslate2 through `ct2rs` running Helsinki-NLP's OPUS-MT sv↔en models converted to int8, behind the SV-05 `TokenGuard`; ADR-0005). Models aren't bundled: they're assets of the GitHub pre-release `models-opus-mt-2026-11-01` (D-9, M1-TR-7) and the manifest has their URLs, but **the repository is private until the first app release** (owner, session 9), so ⬇ gets 404 and the lens then shows the install command instead; `python scripts/models/opus_mt.py` builds and installs the same files offline and is the install path for now. `--demo` / `POLYGLASS_DEMO=1` keeps all mocks. Since session 10 the lens fits text from the source size down to 9 px, shows the original while left Alt is held, copies (⧉, Ctrl+Alt+C), swaps direction (⇄, Ctrl+Alt+S), has the HK-01 hotkeys and keeps its position, click-through and target in `settings.toml` (M1-APP-6/7/9, SET-01). Change detection is tuned on real captures (session 10): a caret blink doesn't re-run OCR, an edited character does, and live mode over static content costs ≈ 1% of a core in a release build (M1-CAP-6). Fixture images are rendered from `truth.json` at test time (100–200%, light/dark; font sizes are em sizes since session 9) and Windows OCR is gated on them and on the same text drawn as GDI ClearType screen text: OCR-04 and SV-03 pass in full on screen text; on the synthetic images only SV-03's repeated-misread clause fails (M1-OCR-6). 183 tests green (10 ignored: 3 measurements, 4 real-model tests that need the converted models, 2 tests against the GitHub release (fail while the repo is private), the synthetic SV-03 letter gate), clippy `-D warnings`, fmt and `cargo deny check` clean (Windows only so far). Building needs CMake and a C++ compiler (README). ADR-0002 (click-through), ADR-0003 (capture) and ADR-0005 (translation) are verified on Windows only.
- **Dev machine used so far:** Windows 11 x64, Rust 1.99, Node 24, Python 3.12 (no Pillow), no Tesseract, VS 18 Community (MSVC 14.51 with its bundled CMake 4.3 and Ninja; not on the normal `PATH`). Windows OCR languages installed: `en-US`, `en-GB`, `sv-SE` (Swedish was added after session 2; the real "Swedish missing" path is now tested with an uninstalled tag instead).
- **Monitors on this machine:** 2560×1440 at 100% (primary) and 2160×3840 at 150% (above-left, negative coordinates), which is useful for CAP-04 and mixed-DPI checks.
- **Git:** `main` and the tag `models-opus-mt-2026-11-01` are pushed to `origin` (https://github.com/shashankshirol/polyglass) since session 9; the owner made the repository private after that and keeps it private until the first app release. CI ran for the first time on that push.
- **Next up (suggested order, each ≈ one session):**
  1. M1-APP-8's rest: the model manager (MDL-01/03/05) replacing the toolbar ⬇, and the first-run wizard (MDL-06, SV-01). Settings (SET-01) are done.
  2. M0-FIX-4 eval sets, then a `pg-bench` translation stage with the real backend (PERF-01/06 end to end).
  3. M0-SPK-1/SPK-2/SPK-3 on macOS/Linux, including the first CTranslate2 build there (the CI jobs for it have never run).
- **Known follow-ups inside done work** (not separate tasks yet):
  - `deny.toml` allows BSL-1.0 since session 10 for `clipboard-win` and `error-code` (arboard's Windows backend, REN-05). It is permissive and OSI-approved. If the owner prefers not to allow it, the copy can call the Win32 clipboard directly (more `unsafe`).
  - macOS: the hold-Option key state (REN-04) has no source yet (`hold.rs` runs on Windows only), and the ⌘⌥ hotkeys weren't tried. Do both with M1-CAP-3.
  - Change detection (M1-CAP-6): a typed narrow glyph (`l`, `I`, `|`, `1`) alone looks like a caret and waits for the next change; spinners and progress bars re-run OCR on every re-check (≈ 8–10 ms, translation cached). Over video, OCR misreads the moving background differently in each frame and each misread is translated (≈ 34–83 ms per cycle). Ignoring blocks that change on every cycle could save that. Retune when ScreenCaptureKit/X11 capture land (they may not be bit-exact).
  - `langid` scores 100% on `mixed-sv-en`, but it was tuned on that same fixture. Add a held-out set before trusting the SV-04 ≥ 90% gate.
  - `reqwest` uses rustls with the default **aws-lc-rs** provider. It built fine here, but check its binary size (SIZE-01) and build needs on macOS/Linux/arm64; switch to `ring` if needed.
  - An OCR call already running when the lens moves runs to completion and its result is discarded. OCR never *starts* while moving (ADR-0001).
  - The pipeline queries OCR languages through `recognize` errors on each cycle, and `WindowsOcr` queries `AvailableRecognizerLanguages` on each `recognize` that names a language. That is inside the ≈19 ms OCR p50, so there is no cache yet.
  - `WindowsOcr` reports confidence 1.0 for every line (Windows OCR has none), so the OCR-05 threshold never hides Windows OCR lines.
  - Capture recovery after monitor hot-plug, resolution change or device loss is untested (ADR-0003); errors surface as `cap-error`.
  - The WGC tests (`crates/pg-capture/tests/wgc.rs`) open real windows and need a desktop session with WGC, so they may fail on GitHub's Windows runners. Decide between skipping and a self-hosted runner when CI first runs.
  - Tauri is on the 2.12 line since session 5 (D-6: MSRV 1.90): crate 2.12.5 (`tauri = "2.12"`, `tauri-build = "2.7"`), npm `@tauri-apps/api` / CLI 2.12.4. Keep npm and crate on the same minor version when moving on (the CLI warns otherwise). Tauri 2.12 uses `windows` 0.62, so the workspace moved to 0.62 too (one copy).
  - The click-through poller doesn't check mouse-button state (ADR-0002); a drag-selection that crosses into the interior flips to click-through mid-gesture.
  - Demo mode's synthetic desktop uses the starting monitor's scale for all monitors (mock only).
  - The MSRV (1.90 since D-6) is checked locally with `cargo +1.90 check --workspace --all-targets`; CI has an `msrv` job since session 8 (never run). `pg-capture` is declared with `default-features = false` in `[workspace.dependencies]` (Cargo 1.85 rejected members turning them off) and `pg-bench` asks for `fixture`.
  - `ct2rs` (0.10.3, bundles CTranslate2 4.8.4) has one maintainer; it's a thin MIT binding that can be vendored. Keep the Python converter (`scripts/models/requirements.txt`) on the CTranslate2 version ct2rs bundles, and re-run the real-model tests (`cargo test -p pg-ct2 --release --test real_models -- --ignored --test-threads 1`) on every upgrade.
  - Windows build: `.cargo/config.toml` sets `+crt-static` and `CMAKE_GENERATOR_<msvc target> = Ninja`. With the Visual Studio generator cmake-rs drops `/O2` and CTranslate2 runs ≈ 13× slower (ADR-0005). A `RUSTFLAGS` or `CMAKE_GENERATOR` env var overrides these; the README says so. The static CRT is now used by the whole workspace incl. the app; `tauri build` with bundling (WiX/NSIS) wasn't run with it.
  - macOS/Linux/arm64: CTranslate2 wasn't built there. ruy is used everywhere; on macOS `accelerate` may be faster (untested).
  - The OPUS-MT models translate sentence by sentence (`pg_ct2::split_sentences`, a small rule set with Swedish/English abbreviations). Spot checks show typical slips (`spara dem` → `rädda dem`, `osparat ändringar`); quality needs SV-07 (M0-FIX-4).
  - After an offline install with `scripts/models/opus_mt.py` the app must be restarted (models load at startup; a ⬇ download swaps them in live). The toolbar ⬇ download has no cancel button or retry backoff (pg-models supports cancel), and a model that fails to load is logged and shown as "missing".
  - A new models release needs new manifest URLs (a new tag) before the tag is pushed; `release_assets.py` refuses a tag the manifest doesn't name, and the workflow never replaces a release. The workflow converts on `windows-latest` (where the output pins were recorded); whether a Linux conversion gives the same bytes is untested.
  - `pg-bench` still uses `MockTranslator`, so PERF-01/06 aren't measured end to end there. The live release app (session 8) logged an uncached cycle of 117 ms (capture 0.8 + OCR 8.6 + translation 107.6 ms) for the `lens-read.ps1` window (one Swedish and one English line); a cached repeat took 10 ms.
  - `deny.toml` allows three licenses nothing uses (`BSD-2-Clause`, `Unicode-DFS-2016`, `OpenSSL`; `cargo deny` warns "license-not-encountered"). They predate session 8; drop them if they stay unused on all targets.
  - The ADR-0004 models are still on this machine (location in the session 8 log); the app ignores them.
  - Windows OCR drops isolated short words: "Save" alone at 12 px is missed at 100% even with page margins, and in a crop with ≤ 1/3 of a line gap above and below, "Save" and "Save changes?" vanish at 200% too. 8–16 px of background padding around the frame brought the 200% case to 0% CER (session 6). Consider padding frames inside `WindowsOcr` (the lens edge can sit close to text).
  - Fixture images are grayscale anti-aliased with no hinting (`ab_glyph`). Real screen text uses hinting and ClearType: on the same layout drawn by GDI ClearType (`crates/pg-capture/tests/cleartype.rs`) Windows OCR reads å/ä/ö 0/480 wrong vs 9/480 (ä→a×8, 12 px at 100%) on the synthetic images. The screen-text gates simulate 125/150/200% on a 100% monitor (GDI draws at the scaled size) and use GDI only, not DirectWrite (browsers, WinUI); they open windows like the WGC tests, so they need a desktop session (see the CI note above).
  - Numbers recorded before session 9 for `pg-testkit`-rendered text (M1-OCR-1, M1-OCR-5, M1-CAP-5, the M1-OCR-6 table) used ab_glyph's cell height as the size: their "12–16 px" is 9–12 px em. They're kept as history; re-measure before relying on them.
  - While the repository is private, `cargo test -p pg-models --test github_release -- --ignored` fails (404). The lens handles it: a ⬇ that gets HTTP 401/403/404/410 hides ⬇ for the run and shows the install command (`mdl-error-unreachable`, test `mdl03_unreachable_release_falls_back_to_the_install_command`; checked in the release app against the private repo). The release app can't download the models until the repo is public: make it public before the first app release, or move the models to another host.
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
- [x] D-8 Translation engine. **Decided (2026-11-01): "go with your recommendation of Move to CTranslate2 with OPUS-MT models. Skip the m2m100 lead."** → ADR-0005, M1-TR-6. The research that led to it:
  - The engines that run the previous models (ADR-0004) are young or lightly maintained; details in ADR-0005 "Context".
  - CTranslate2 (OpenNMT) is actively released (v4.8.4, 2026-10) and converts MarianMT/OPUS-MT, M2M-100 and NLLB. Session 4 measured it with OPUS-MT on this machine: 26.5 ms short, 199 ms paragraph, 200 MB RSS, 162 MB of models for both directions, CC-BY-4.0, which we'd self-host after conversion. The `ct2rs` binding has one maintainer.
  - `facebook/m2m100_418M` (MIT, 100 languages incl. sv/de/en): 1.94 GB in fp32, roughly 0.4–0.5 GB as int8 (an estimate). Not measured; expected to be much slower than the per-pair models and to break SIZE-05/PERF-01.
  - Also possible, not measured: Hugging Face `candle` (pure Rust; has Marian, no M2M-100 in its model list) and ONNX Runtime via `ort`.
  - Engines sit behind `Translator`, so a switch touches only the backend crate, the manifest and `live.rs`.
- [x] D-9 Where to host the converted OPUS-MT CTranslate2 model files (ADR-0005, MDL-02). **Decided (2026-11-01): "let's use the github releases"**: assets of a GitHub Release of `github.com/shashankshirol/polyglass` (remote `origin`, added by the owner). Work tracked as M1-TR-7.

---

## M0 — Spikes and foundations

### Repository and tooling
- [x] M0-REPO-1 Cargo workspace with `crates/pg-{core,capture,ocr,translate,pipeline,models,bench}`.
- [x] M0-REPO-2 `.gitignore`, `rustfmt.toml`, workspace lints, `deny.toml`.
- [~] M0-REPO-3 CI skeleton `.github/workflows/ci.yml`: fmt, clippy `-D warnings`, tests on Windows/macOS (arm64 + x86_64)/Ubuntu, `pg-bench` smoke run, `cargo deny` (QUAL-01, SEC-05 skeleton). *First run in session 9 (push of `main`, run 41284802134): see the session 9 log for results. Since session 9 (owner: the repo is private, Actions minutes), pushes and PRs run the `test` job on Linux only; Windows and macOS run only on a manual dispatch (Actions → CI → Run workflow, input `os` = all/windows/macos/linux). fmt, msrv, ui and deny run on Linux every time. `cargo deny check` passes locally (cargo-deny 0.20.2, two documented advisory ignores, D-6). MSRV job since session 8.*
- [x] M0-REPO-4 Docs skeleton: README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, GOVERNANCE, PRIVACY, docs/ARCHITECTURE.md, docs/adr/0000-template + ADR-0001 (OSS-03 skeleton). *Contact e-mails are `.invalid` placeholders.*
- [ ] M0-REPO-5 Add LICENSE-MIT / LICENSE-APACHE once D-2 is decided.
- [~] M0-REPO-6 Repo history + public remote. *Done: local repo + initial commit `8cd14de`; public remote `origin` (github.com/shashankshirol/polyglass), `main` pushed in session 9. Not done: real contact addresses, `.gitattributes` (global `core.autocrlf` converts files to CRLF on Windows checkouts).*
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
- [~] M0-SPK-3 On-device translation engines with sv↔en models: build complexity, size, latency, memory on all OSes. → ADR-0004, superseded by ADR-0005 (CTranslate2 + OPUS-MT). *Windows done: ADR-0004 measured CTranslate2 (`ct2rs` 0.10.3, ruy) + OPUS-MT int8 against a pure-Rust option; ADR-0005 (D-8) chose CTranslate2, implemented and re-measured in M1-TR-6. Not done: ADR-0004's option A (C++ via `cxx`) wasn't built (running CMake on the downloaded source wasn't permitted in session 4), and nothing was built or timed on macOS/Linux/arm64.*
- [ ] M0-SPK-4 Swedish fixtures + OCR accuracy/latency per engine incl. Vision vs Tesseract `swe` on macOS. → ADR-0005.
- [ ] M0-SPK-5 Wayland portal screenshot + fullscreen Freeze window on GNOME and KDE. → ADR-0006.

### Fixtures (SPEC §5, CC0)
- [~] M0-FIX-1 Ground-truth text for `ui-dialog-sv`, `ui-dialog-en`, `mixed-sv-en`, `numbers-sv`, `numbers-en`, `paragraphs-sv`, `paragraphs-en`, `long-compounds-en` in `tests/fixtures/*/truth.json` (format in `tests/fixtures/README.md`). *Done: text, block lists, expected languages. Not done: `bbox` per block, images, a held-out `mixed-sv-en-2` for langid, and a native-speaker review of the Swedish.*
- [x] M0-FIX-2 Fixture image generator (render truth.json to PNG at 100/125/150/200%, light + dark theme, 12–16 px UI fonts). *`pg_testkit::fixture::FixtureImage::render` (Rust, `ab_glyph` + the OS UI font): 600 logical px page, block i in 12–16 px (`i % 5`; em sizes since session 9, before that ab_glyph cell heights, i.e. 9–12 px em), word-wrapped, light `#F3F3F3`/`#1B1B1B` and dark `#202020`/white; returns each line's text and box. **Rendered at test time, not committed**: the font (Segoe UI here) isn't redistributable and CC0 for its renderings is unclear (D-7); each OS is tested on its own UI font; no images to keep in sync. Deterministic per font file (`rendering_is_deterministic`; the font's SHA-256 is recorded). `cargo run -p pg-testkit --bin render-fixtures` writes all 64 PNGs + line boxes (≈ 3 MB) to `target/fixture-images`. Wired in: `FixtureCapturer::from_png`, `pg-bench` (`--scale`, `--theme`), CAP-04 tests, OCR gates. Tests: `fixture::tests` (5).*
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
- [x] M1-CAP-6 Tune change detection (`ChangeConfig`, CAP-03) on real captures; live-mode CPU in a release build (ADR-0003 "Change detection"). *Done in session 10 on Windows.*
  - *Measured with `crates/pg-pipeline/tests/change_wgc.rs`: GDI ClearType text in a real window, WGC capture, every case at all 64 cell-grid alignments, 100/150/200% (`measure_change_detection`, ignored). Static content captures bit-exactly. A caret blink and a changed digit or typed letter both touch 2–12 cells of 8 px, so the old rule (> 0.15% of cells over 6 levels) re-ran OCR on every caret blink at the default 480×200 lens but never saw an edited digit or typed letter at 600×300 and larger. Text typed one letter per re-check was never picked up at all, because each capture became the next reference.*
  - *New rule: a frame differs when a pixel moved by > 24 levels, unless those pixels form a caret (≤ 3 logical px wide, ≥ 2× as tall as wide, ≥ 8 logical px tall), or when a cell's mean moved by > 3 levels outside it (fades). The reference is the last processed frame. Now caret blinks are 0/64, and digit, letter, word, 1–4 px and line scrolls and video are 64/64 at every scale. Cost: 0.5 ms per re-check at 600×300, 2 ms at 1200×600 (release).*
  - *Tests: `cap03_real_captures_ignore_the_caret_and_see_edits` (default run, real windows, ≈ 26 s debug), 9 unit tests in `change.rs`, `cap03_caret_blink_skips_ocr` and `cap03_small_changes_add_up` (`tests/pipeline.rs`).*
  - *Release-app CPU with `apps/desktop/scripts/lens-cpu.ps1` (480×200 lens, 100% monitor, per 10 s):*
    - *static text: 47–188 ms (≈ 1% of a core; debug was 438 ms);*
    - *the real Windows caret blinking in a text box: 0 cycles (the session 9 build ran 5);*
    - *an animation behind text: every re-check processed, 4.3 s of CPU, mostly translating text OCR reads differently off the moving background.*
  - *Not measured: DirectWrite text, block carets (terminals), 150% and larger lenses in the app, macOS/X11 capture.*
- [ ] M1-CAP-3 macOS: ScreenCaptureKit stream, `sourceRect` = lens, excluding own app (CAP-01/02).
- [ ] M1-CAP-4 macOS Screen Recording permission missing/revoked → explanation + open-settings button + auto-resume (CAP-05).
- [x] M1-CAP-5 HiDPI/mixed-DPI fixture tests at 100/125/150/200% (CAP-04, LENS-02). *Real-capture tests at 100% and 150% and across a mixed-DPI monitor edge (`cap04_every_monitor_and_a_region_across_two`); the live lens was checked on both monitors (ADR-0003). Fixture tests at all four scales (`crates/pg-capture/tests/fixture.rs`): `cap04_lens_over_a_line_at_every_scale` (lens in logical px over each separated line of `ui-dialog-en`, light + dark, `LogicalRect::to_physical`; the capture holds exactly that line's ink) and `cap04_windows_ocr_reads_the_lens_at_every_scale` (100 px lens moved down the page, Windows OCR CER ≤ 5%: 3.6/3.3% at 100%, 0–1.8% above). Not tested: real monitors at 125% and 200% (none here).*

### OCR
- [x] M1-OCR-1 Windows OCR engine via `windows` crate (`Windows.Media.Ocr`), lines + boxes + confidence (OCR-01/02). *`pg_ocr::WindowsOcr` (windows 0.62 since session 5, the version Tauri 2.12 uses; no `unsafe`). Line box = union of word boxes in frame pixels; confidence is always 1.0 because Windows OCR has none, so OCR-05 filtering does nothing with this engine. Tests in `crates/pg-ocr/tests/windows_ocr.rs` render text with `pg-testkit` (Segoe UI, grayscale AA): `ocr01_recognizes_rendered_english_lines_with_boxes`, `ocr01_dark_theme_and_pixel_formats` (BGRA/RGBA with padded stride/Gray8), `ocr01_boxes_are_in_frame_pixels_at_any_upscale`. Text is checked with a CER bound (≤ 6%/line), not exact match. PERF-03 (release, `pg-bench --ocr windows`, 600×300, sv): p50 18.9 ms / p95 19.1 ms at 100%; 34.0 / 35.9 ms at 150% (900×450 px).*
- [~] M1-OCR-2 Runtime language query `AvailableRecognizerLanguages` (OCR-06) + SV-02 Windows message/link when `sv-SE` missing. *Done: `WindowsOcr::available_languages()` is queried on every `recognize` that names a language and by `supported_languages()`; a missing language is `Error::OcrLanguageMissing`. Tests: `ocr06_languages_are_queried_at_runtime`, `ocr03_uninstalled_language_is_reported_missing` (real engine, `tlh`), `sv02_swedish_missing_or_recognized` (checks whichever branch the machine is on). **sv-SE is now installed on this machine**, so the real "Swedish missing" branch can't be hit here any more. Lens UI: when Swedish OCR is missing (the Auto-mode notice or `ocr-error-language-missing`), the interior explains where to add it and a toolbar "⚠ SV" button calls `open_ocr_language_settings`, which opens `ms-settings:regionlanguage`. The button is in the toolbar because the interior is click-through. Not verified in the running app: Swedish is installed here, so that state never occurs.*
- [ ] M1-OCR-3 Apple Vision engine (English) + runtime language check (OCR-01, OCR-06).
- [ ] M1-OCR-4 Tesseract 5 engine (bundled on macOS with `swe`+`eng`) (OCR-01, SV-02 mac).
- [x] M1-OCR-5 Upscale small text before OCR (PLAN §10 risk mitigation). *`WindowsOcr` enlarges frames 2× (bilinear) below 200% scale; `with_upscale(k)` forces a factor. Measured on rendered Segoe UI, 5 lines (3 en, 2 sv), 600×300, release: exact lines at 1×/2×/3× — 11 px 0/0/1, 12 px 0/2/2, 13 px 1/2/2, 14 px 2/3/2, 16 px 2/3/2, 20 px 5/5/5; at 1× small text drops whole words. OCR time 1× ≈ 8–17 ms, 2× ≈ 17–26 ms, 3× ≈ 30–35 ms. Remaining misreads at 2× are mostly 5→S and rn→m (`ändringama`). Tuned on synthetic text only; recheck on real captures (ClearType).*
- [x] M1-OCR-6 Accuracy gates on fixtures: OCR-04 (en), SV-03 (sv, å/ä/ö). *Done in session 9 on Windows; two calls for the owner to confirm, below.*
  - *Screen-text gates (`crates/pg-capture/tests/cleartype.rs`, run by default, ≈ 40 s debug): each fixture laid out like `FixtureImage::render`, each 600×300-logical lens strip drawn with GDI ClearType Segoe UI in a real topmost window on the primary monitor, captured with WGC, read by Windows OCR (default upscaling; 125/150/200% simulated by drawing at the scaled size and tagging the frame with that scale). `ocr04_english_screen_text_cer` **passes**: CER 0.25% (25/10,152). `sv03_swedish_screen_text` **passes in full**: CER 0.07% (13/17,488), å/ä/ö 0/480, no repeated misread. Identical in debug and release and across runs.*
  - *Rendered-image gates (`crates/pg-ocr/tests/accuracy.rs`): `ocr04_english_fixture_set_cer` passes, CER 0.25%; `sv03_swedish_fixture_set_cer` passes, CER 0.13%, å/ä/ö 1.88% (regression bound now ≤ 5%); `sv03_swedish_letters` (`#[ignore]`d) fails only on its repeated-misread clause: ä→a ×8 (+ Ä→A ×1), all in 12 px text at 100% except one at 200%. So the remaining misreads come from the synthetic renderer (grayscale, unhinted), not from Windows OCR on real text.*
  - *Call 1 — font size: `TextRenderer` used to pass px to `ab_glyph`'s `PxScale`, which is the ascent-to-descent height (≈ 1.33 em for Segoe UI), so the fixtures' "12–16 px" was 9–12 px em (6.8–9 pt), smaller than SPEC's "UI text 12–16 px" read as a CSS font size. It now draws at the em size (`TextRenderer::scale`). With the old sizes, the screen-text gate gave CER 1.33%, å/ä/ö 1.04% but ä→∅ ×3 (a dropped word and "Här är" → "HÜ Sr", all 9 px em at 100%), i.e. SV-03 would still fail its repeated-misread clause.*
  - *Call 2 — which rendering the SV-03 letter gate uses: the screen-text one, since it is what the lens sees; the synthetic one stays as an ignored measurement.*
  - *Old session 6 numbers (cell-height sizes, synthetic images): English CER 0.80%, Swedish CER 0.82%, å/ä/ö 7.3% (ä→å ×16, ä→a ×8, …).*

### Translation and models
- [x] M1-TR-1 On-device backend implementing `Translator`, `is_local() == true` (TR-01). *Since session 8 (M1-TR-6): crate `pg-ct2`, `Ct2Translator` (id `ctranslate2`), CTranslate2 int8 on Tokio's blocking pool, blocks split into sentences and sent as one batch. Tests: `without_models_every_pair_is_model_missing`, `a_broken_installed_model_is_an_error_not_a_panic`, `an_entry_without_the_model_files_is_rejected`, `sentences::tests` (3); real models: `tr1_tr3_both_directions_loaded_and_translate` (ignored, run manually: passes). The app uses it in live mode, manually verified in the release app (`cargo build -p polyglass-desktop --release --features tauri/custom-protocol`), models installed by `python scripts/models/opus_mt.py`, read with `lens-read.ps1`: "Vill du spara ändringarna innan du stänger?" → "Do you want to save the changes before closing?", the English line passed through. Without the models the lens showed the install command and no ⬇.*
- [x] M1-TR-2 Model manifest entries for sv→en, en→sv with SHA-256/licenses (MDL-02, SIZE-05). *`opus-mt-sv-en`, `opus-mt-en-sv`: 6 files each with size + SHA-256, CC-BY-4.0 + attribution, 169.6 MB together. **No URLs yet** (D-9): the entries are import-only, installed by `scripts/models/opus_mt.py` (pinned converter, input zip and output SHA-256 pinned, reproducible) through `import-models` (MDL-04). Tests: `bundled_manifest_has_both_swedish_directions` (incl. SIZE-05 ≤ 200 MB, CC-BY-4.0, attribution); `mdl04_tr2_converted_models_import_and_verify` (ignored, passes). URLs since session 9 (M1-TR-7): assets of the GitHub pre-release `models-opus-mt-2026-11-01`; `bundled_manifest_has_both_swedish_directions` checks them.*
- [x] M1-TR-3 Both directions loaded → instant swap (SV-01). *`Ct2Translator::load_installed` loads every installed direction at startup (0.27 s for both in a release test, off the UI thread in the app). `tr1_tr3_both_directions_loaded_and_translate` alternates sv→en / en→sv without reloading: 58–86 ms per pair of short strings (two runs). The toolbar ⇄ and Ctrl+Alt+S swap the target live since session 10 (M1-APP-9).*
- [x] M1-TR-4 Preserved-token gate on `numbers-sv`/`numbers-en` with the real backend (SV-05). *On OPUS-MT (session 8): `sv05_placeholders_survive_opus_mt` (ignored) shows `{n}` placeholders come back unchanged in all 20 masked blocks, so the masking is unchanged; `tr4_sv05_numbers_fixtures_keep_preserved_tokens` (ignored): all 20 blocks pass `check_preserved`, 5 needed the masked pass (all sv→en), 0 fell back to the source. Example: `Totalt att betala: 1 234,50 kr` → `Total to pay: 1,234.50`. The guard logic is also unit-tested with a mock (`guard::tests`, 8 tests).*
- [x] M1-TR-6 Move on-device translation to CTranslate2 + OPUS-MT (ADR-0005, D-8). *Done in session 8 on Windows:*
  - *Crate `pg-ct2` (`ct2rs` 0.10.3, ruy + SentencePiece, no `unsafe` in our code): any installed `ctranslate2` manifest entry is loaded, no language assumptions (M3-8/M3-9 need only manifest + script entries). Threads per model: `Options::default_threads()` = half the logical CPUs, 1..4.*
  - *`scripts/models/opus_mt.py` + hash-locked `requirements.txt` (ctranslate2 4.8.4, numpy 2.5.4, pyyaml 6.0.3): official `swe-eng/opus+bt-2021-04-30` and `eng-swe/opus+bt-2021-04-14` zips (newest pair-specific releases, SHA-256 pinned) → int8; output SHA-256 pinned, identical across two fresh runs. One command converts and installs into the app's store.*
  - *`pg-models`: file `url` is optional (import-only entries, `ModelError::NotHosted`), `attribution` field (required for CC-BY), `import-models` binary. The lens shows the install command instead of ⬇ while the models are import-only.*
  - *Measured (release, ADR-0005): p50 over two runs, 4 threads: sv→en short 27.6–28.6 ms, paragraph 156–212 ms, ui-dialog-sv 147–198 ms; en→sv short 27.9–37.5 ms, paragraph 154–209 ms, ui-dialog-en 138–185 ms; RSS 211 MB with both loaded, 275 MB after the runs. Live release app: both models loaded in 0.25 s at startup; uncached cycle 117 ms (OCR 8.6 + translation 107.6 ms), cached 10 ms; `polyglass.exe` working set 294 MB (WebView2 not counted; 126 MB before). The log had no screen text (PRIV-07).*
  - *Build: plain `cargo build` / `npm run tauri dev` from a normal PowerShell work once CMake and Ninja are on `PATH` (README one-time setup: `winget install Kitware.CMake Ninja-build.Ninja`); `.cargo/config.toml` sets `+crt-static` and Ninja for MSVC targets. CI installs CMake/C++ (and Ninja on Windows) on all OSes and has an MSRV job; never run (no remote).*
  - *Removed the ADR-0004 backend: its crate and tests, its engine dependency, `flate2`, its two manifest entries and engine id, its UI text and docs (commit `84f9dad`). ADR-0004 stays as history. `deny.toml`: `MPL-2.0` is still needed (cssparser, selectors, dtoa-short, option-ext via Tauri), only its comment changed.*
- [!] M1-TR-7 Host the converted models on GitHub Releases (D-9, MDL-02/03). **Blocked: the owner made the repository private after the release and keeps it private until the first app release (session 9)**, so the release URLs answer 404 without a GitHub login; after ⬇ the lens falls back to the install command. Everything below was done and verified while it was public; it works again as soon as the repo is public. *Done in session 9 (owner approved pushing `main` and the tag, which creates the release):*
  - *Release: https://github.com/shashankshirol/polyglass/releases/tag/models-opus-mt-2026-11-01, a pre-release (never "latest") titled "Models: …", notes say it's a models release and carry the attribution, provenance (input zips + SHA-256, commit) and a file table. 15 assets: `<id>.<file>` for both models (a release's assets share one namespace), `LICENSE-CC-BY-4.0.txt`, `ATTRIBUTION.md`, `SHA256SUMS`. GitHub's asset digests equal the manifest's SHA-256.*
  - *How: `.github/workflows/models-release.yml` runs on a pushed `models-opus-mt-*` tag. A read-only `windows-latest` job runs `opus_mt.py --no-install` (pins checked) and `scripts/models/release_assets.py --tag <tag>` (manifest URLs must name the tag and `<id>.<file>`; size + SHA-256 against the manifest; writes license, attribution, sums, notes). A `contents: write` job runs no project code: `sha256sum --check`, refuses an existing release, `gh release create --verify-tag --prerelease --latest=false`. Run 41284805953 took ≈ 2 min.*
  - *Manifest: every `opus-mt-*` file has its `releases/download/models-opus-mt-2026-11-01/<id>.<file>` URL.*
  - *Downloader: follows redirects only to https (or loopback http for tests), ≤ 10; a refused redirect is `InsecureUrl`. Tests: `mdl03_follows_redirects_and_resumes_through_them`, `sec03_redirect_to_plain_http_is_refused`; against the real release (ignored, pass): `mdl02_release_redirects_to_an_https_asset_host` (302 → `https://release-assets.githubusercontent.com`, SHA-256 verified), `mdl03_resume_with_range_through_the_release_redirect` (resumed at byte 663,832 of 1,327,665).*
  - *Manually verified in the release app (`npm run tauri build -- --no-bundle`), with the installed models moved aside: the lens showed "aren't downloaded yet" and ⬇ (169.7 MB); ⬇ (invoked through UI Automation) downloaded both directions in ≈ 10 s, the files were byte-identical to the imported ones, the models loaded in 292 ms without a restart, ⬇ disappeared, and `lens-read.ps1` showed "Do you want to save the changes before closing?" with the English line passed through. The log had no screen text.*
  - *PRIVACY.md names GitHub (and its download servers) as the model host; README §3, ARCHITECTURE, ADR-0005 updated.*

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
  - *~~Restore of last size/position~~ done in session 10 (SET-01, `lens01_restores_only_onto_a_monitor_that_is_there`; checked in the release app: moved 50 px, hidden, restarted → same spot). There is only one mode so far.*
  - *The max size (monitor work area) is set only at launch, not when the lens moves to another monitor.*
  - *~~The click-through hotkey~~ Ctrl+Alt+T since session 10 (M1-APP-9).*
  - *The toolbar's click-through toggle is implemented, but its effect was not checked with input.*
- [~] M1-APP-3 Toolbar with accessible controls + privacy indicator (LENS-05, PRIV-05). *Stub done:*
  - *Every control is a `<button>` with an accessible name.*
  - *The privacy indicator shows a lock or a cloud with the provider name, driven by `LensUpdate.backend.local`.*
  - *Pause/resume parks the pipeline (verified: lens cleared, CPU drops).*
  - *Close hides the lens to the tray.*

  *Not done:*
  - *A mode badge: "DEMO" with `--demo`, "MOCK MT" in live mode until M1-TR-1.*
  - *Source/target/mode/settings are `aria-disabled` placeholders (the target shows EN/SV; ⇄ and ⧉ work since session 10).*
  - *Keyboard Tab order and screen-reader output not tested.*
- [~] M1-APP-4 Tray icon, no taskbar/Dock entry, show/hide hotkey parks the pipeline (LENS-01, LENS-06). *Done:*
  - *Tray icon with Show/hide and Quit, plus left-click toggle.*
  - *No taskbar entry (verified).*
  - *Hiding (toolbar close or Alt+F4) parks both the pipeline and the click-through poller (verified 0 ms CPU over 10 s).*

  *Not done:*
  - *The tray menu wasn't clicked in testing.*
  - *~~The global hotkey~~ Ctrl+Alt+L since session 10 (M1-APP-9).*
- [~] M1-APP-5 Wire `pg-pipeline` driver to lens events; render `LensUpdate` in Overlay mode (MODE-01, REN-01). *Done:*
  - *Move/resize/scale/show/hide/pause events go through `state::Lens` into the `DriverHandle`.*
  - *The interior region is computed with `LogicalRect::to_physical`.*
  - *`lens-update` events are drawn as positioned DOM blocks with the sampled bg/fg (`#rrggbb` only) and `textContent` only.*
  - *A basic 16→9 px shrink-to-fit (full REN-02 is M1-APP-6).*
  - *Verified end to end with `--demo` (mock engines). Test: `demo_pipeline_translates_swedish_and_passes_english_through`.*

  *Since session 3 the default (non-demo) pipeline on Windows is real capture + Windows OCR (`live.rs`), checked with `apps/desktop/scripts/lens-read.ps1` on the 100% and 150% monitors. Not done: real translation (M1-TR-1).*
- [x] M1-APP-6 Font fitting down to 9 px + grow + tooltip (REN-02). *Done in session 10.*
  - *`RenderedBlock.line_height` (mean source line height, logical px) is where fitting starts (capped to 9–48 px). `apps/desktop/ui/src/fit.ts` picks the largest whole px size from there down to 9 px at which the text stays inside the source box. Text that still overflows grows the box downward, cut at the interior's bottom edge, with the full text in the block's tooltip.*
  - *Tests: `npm test` (`ui/test/fit.test.ts`, Node's test runner, no new dependencies; added to the CI `ui` job), `line_height_is_the_mean_of_the_lines` (pg-core), and line-height asserts in `tests/pipeline.rs` incl. 150%.*
  - *Checked in Chromium on the plain-browser preview at 480×200: 18 px boxes fit at 15 px (line-height 1.2); a long block went to 9 px, grew to two lines with its tooltip, and was capped at the interior's edge. The live release app shows the translations (font sizes weren't read there).*
  - *Gaps: the tooltip can only be hovered with click-through off (Panel view is M2-2); a grown box can cover the block below it.*
- [x] M1-APP-7 Hold Alt/Option to show original (REN-04); copy hotkey (REN-05, UTF-8 round-trip test). *Done in session 10 on Windows.*
  - *REN-04: `hold.rs` reads the key state at ~30 Hz while the lens is visible (it is click-through and rarely focused, so there are no key events) and sends `lens-original` while left Alt alone is held. Right Alt and Ctrl combinations don't count, so AltGr (Ctrl + right Alt) and the Ctrl+Alt hotkeys never flash it (SV-09). It only reads, so other apps get every key. Tests: `ren04_left_alt_alone_shows_the_original`, `sv09_altgr_and_hotkeys_do_not_show_the_original`.*
  - *REN-05: toolbar ⧉ → `copy_translation` (no webview input) → `LensUpdate::copy_text` (displayed blocks in reading order, one per line) → clipboard via `arboard` (text only). Tests: `ren05_copy_text_is_the_displayed_blocks_in_order`, `ren05_clipboard_round_trips_swedish_text` (real clipboard, puts the old text back).*
  - *Live, release app, through UI Automation and real key events: translation → left Alt held: the Swedish source → released: translation; AltGr: no change; ⧉: both blocks on the clipboard in order.*
  - *The Ctrl+Alt+C hotkey (M1-APP-9) copies the same text; checked in the release app (å/ä/ö intact on the clipboard). macOS (Option) has no key-state source yet (Known follow-ups).*
- [~] M1-APP-8 Settings TOML in config dir (SET-01); model manager UI (MDL-01/03/05); first-run wizard (MDL-06, SV-01). *Settings done in session 10; the model manager and the wizard are not started.*
  - *`settings.rs`: `settings.toml` in the app config dir (`%APPDATA%\org.polyglass.desktop`) with `[lens]` x/y (physical), width/height (logical), click_through and `[translation]` target (`en`/`sv`). Starts with a comment line. Missing keys take defaults, unknown keys are ignored, a UTF-8 BOM is accepted, and a file that doesn't parse is renamed `settings.toml.bad` (defaults used). Atomic writes (temp + rename) when a setting changes, when the lens hides and at exit.*
  - *The lens goes back to its saved spot when the toolbar would land on a monitor, else it is centred as before. Click-through and the target come back.*
  - *Tests: `set01_round_trips_through_readable_toml`, `set01_missing_file_and_keys_take_defaults`, `set01_reads_a_file_saved_with_a_byte_order_mark`, `set01_broken_file_is_kept_aside`, `store_saves_only_changes`, `lens01_restores_only_onto_a_monitor_that_is_there`.*
  - *Release app, through UI Automation and lens-probe: moved, toggled click-through off, hid → the file had both; restart → same position, click-through off and not click-through over the interior; a broken file → `.bad` kept, centred, click-through on. The check removed the file afterwards (there was none before).*
  - *Not done: the model manager (MDL-01 catalog with size/license/source, MDL-03 progress + cancel + resume, MDL-05 delete + disk usage) and the first-run wizard (MDL-06, SV-01). The toolbar ⬇ and the install-command fallback (session 9) remain the interim.*
- [x] M1-APP-9 Default hotkeys (HK-01) + Swedish-layout AltGr check (SV-09). *Done in session 10 on Windows.*
  - *`hotkeys.rs` through `tauri-plugin-global-shortcut` 2.4 (registered from Rust; the webview has no permission for the plugin): Ctrl+Alt+L show/hide, Ctrl+Alt+C copy, Ctrl+Alt+S swap, Ctrl+Alt+T click-through (⌘⌥ on macOS). A key another app holds is logged and skipped (HK-02 reporting is M4-2). Ctrl+Alt+F comes with Freeze mode (M2-3).*
  - *Swap (toolbar ⇄, `swap_languages`, or the hotkey) flips the target en ↔ sv, saves it and invalidates the pipeline; both directions stay loaded (SV-01). The toolbar shows the target (EN/SV).*
  - *SV-09: `sv09_hotkeys_type_nothing_with_altgr_on_the_swedish_layout` reads the installed Swedish layout (never installs one; skips without it) with `ToUnicodeEx`: AltGr+L/C/S/T type nothing, AltGr+2 = @ and AltGr+E = € as the control. Also `hk01_defaults_map_back_to_their_actions`.*
  - *Release app, real key events: Ctrl+Alt+S → target SV, the English line translated into Swedish and the Swedish line passed through; Ctrl+Alt+C → clipboard; Ctrl+Alt+T → click-through off/on; Ctrl+Alt+L → hidden/shown. With the hotkeys registered, AltGr+2 3 4 7 8 9 0 + typed `@£${[]}\` into another app's text box.*
  - *Not run: macOS and Linux.*
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
- [ ] M3-8 Configurable language pairs (MDL-07, D-5): pairs come from settings + the model catalog instead of the hard-coded sv/en (`SourceLang::AutoSvEn`, `langid` sv/en scorer, `live.rs`, the Swedish-only first-run download in `models.rs`; `pg-ct2` already loads any installed pair); toolbar source/target pickers; OCR language follows the source (OCR-06). Could move earlier if it simplifies M3-9.
- [ ] M3-9 German → English on-device (SPEC §3, D-5): OPUS-MT `deu-eng` in `scripts/models/opus_mt.py` + a manifest entry (pick the newest release that fits, record its license and SHA-256; transformer-big releases are ≈ 4× larger), German OCR (Windows `de-DE` OCR feature, Tesseract `deu` elsewhere), `de-en` fixtures with ä/ö/ü/ß, CER gate.

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

### 2026-11-01 — Session 10 (M1-CAP-6 change detection + CPU; M1-APP-6, 7, 9; SET-01)
- Commits on `main`, none pushed: `c71bfff` caret-aware change detection; `2d98211` `lens-cpu.ps1`; `2310878` ADR-0003/CHECKLIST for M1-CAP-6; `06414ef` + `44bd2e2` font fitting (M1-APP-6); `b97de53` + `dfcb50c` hold Alt / copy (M1-APP-7); `a81e532` checklist; `e11280a` settings.toml + lens restore (SET-01); `4409a2a` BSL-1.0 in `deny.toml`; `0618663` hotkeys + swap (M1-APP-9); then this log.
- M1-CAP-6 (new ID): measured change detection on real WGC captures of GDI ClearType text (`tests/change_wgc.rs`, all 64 grid alignments, 100/150/200%). The 0.15%-of-cells rule ran OCR on every caret blink at the default lens and missed edited digits/typed letters in 600×300+ lenses; captures are bit-exact. Replaced by a per-pixel rule that recognises the caret by shape, compared against the last processed frame. Release-app CPU: ≈ 1% of a core over static text (47–188 ms / 10 s), 0 OCR cycles with a real blinking caret (session 9 build: 5). Numbers and limits in ADR-0003 "Change detection".
- M1-APP-6: REN-02 fitting from the source line height down to 9 px, then grow + tooltip (`ui/src/fit.ts`, `npm test` with Node's test runner, added to the CI `ui` job).
- M1-APP-7: hold left Alt for the original (key-state poller, AltGr excluded); ⧉ copy via `arboard`. M1-APP-9: Ctrl+Alt+L/C/S/T, swap en ↔ sv, SV-09 layout test. All three checked in the release app with UI Automation and real key events.
- M1-APP-8: settings only (`settings.toml`, lens position/size/click-through/target restored). The model manager and the first-run wizard are not started.
- Verified: fmt, clippy `-D warnings`, 183 tests (10 ignored), `cargo +1.90 check --workspace --all-targets`, `cargo deny check`, `npm run build`, `npm test`, all locally on Windows.
- For the owner: `deny.toml` now allows BSL-1.0 (arboard's `clipboard-win`/`error-code`); say if you'd rather not (Known follow-ups). The two M1-OCR-6 calls from session 9 are still open.
- Gotchas:
  - Python heredocs in Git Bash still turn `\\` into `\` (a `"\\n"` became a real newline in Rust source). Put edit scripts in a file, or use the Edit tool.
  - The Browser pane's preview tab has a 0×0 viewport while the pane is hidden; set one with `resize_window` before measuring layout.
  - Windows' foreground lock: a test window from a background process can't take the focus by itself. A real click through the click-through interior works, but wait ≥ 100 ms after moving the cursor so the 30 Hz poller has flipped.
  - `Process.TotalProcessorTime` moves in 15.6 ms steps; CPU over 10 s varies ± 1 tick between runs.
  - The app's settings live in `%APPDATA%\org.polyglass.desktop\settings.toml`. The live checks removed the file they created; there was none before this session.
- Next: the rest of M1-APP-8 (model manager + first-run wizard), then "Next up".

### 2026-11-01 — Session 9 (M1-TR-7: models release; M1-OCR-6)
- Commits: `f52ae4e` downloader follows redirects to https only; `eec975a` models release workflow + `scripts/models/release_assets.py`; `a6737c5` manifest URLs + real-release tests + PRIVACY/ARCHITECTURE/ADR-0005; `200d429` README/CHECKLIST for the release; `ae5c8d6` fixture text at em size; `133c7a9` screen-text OCR gates; then this log.
- Outward-facing, each approved by the owner in the session: pushed `main` (then `a6737c5`) and the annotated tag `models-opus-mt-2026-11-01`, which ran `.github/workflows/models-release.yml` (run 41284805953, ≈ 2 min, success) and created the pre-release with 15 assets. The commits after `a6737c5` are **not pushed**. Afterwards the owner made the repository private and chose to keep it that way, so M1-TR-7 is `[!]` (anonymous downloads get 404).
- Verified: the runner's conversion matched the pinned SHA-256s (the workflow stops otherwise); GitHub's asset digests equal the manifest; redirect to `release-assets.githubusercontent.com` (https) and Range resume through it (`github_release` tests, while public); ⬇ → download (≈ 10 s) → models loaded live (292 ms) → translation in the release app. fmt, clippy `-D warnings`, 161 tests (9 ignored; 162 after the fallback), `cargo +1.90 check --workspace --all-targets`, `cargo deny check` all pass locally.
- First CI run (41284802134, push of `main`): `fmt`, `deny`, `ui` and `msrv` passed; the four `test` jobs were still running when the repo went private and their results weren't checked (the owner can see them in Actions).
- M1-OCR-6: SV-03 passes in full on GDI ClearType screen text (å/ä/ö 0/480); details and the two calls (em-size fonts, screen text as the letter gate) in the item.
- Gotchas:
  - `gh` isn't installed here, so the release is made by the tag-triggered workflow with `GITHUB_TOKEN`; no token touched this machine. Git Credential Manager handled the push.
  - GitHub release assets share one namespace per release: both models have `model.bin`, hence `<id>.<file>` asset names. The `url` can name any asset; the downloader saves under the manifest's `name`.
  - In Git Bash, `export PATH="$(rustc --print sysroot)/bin:$PATH"` puts a `C:\…` path into a `:`-separated PATH and the Tauri CLI fails with "cargo metadata … program not found"; set it from PowerShell instead (README troubleshooting). CMake/Ninja are VS 18's: `…\Common7\IDE\CommonExtensions\Microsoft\CMake\{CMake\bin,Ninja}`.
  - The ⬇ button can be pressed through UI Automation (its name is "Download the Swedish and English translation models (… MB)"; `InvokePattern`), no real mouse input needed.
  - `ab_glyph`'s `PxScale` is the ascent-to-descent height, not the em size; GDI's positive `lfHeight` is the same cell height, negative is the em.
- Left on this machine: `target/opus-mt/release/` (the staged assets, 162 MB) next to session 8's `target/opus-mt/`. The app's store has the downloaded OPUS-MT models (byte-identical to the imported ones); the old `bergamot-*` dirs are still there (session 8 note).
- Later in the session the owner decided to keep the repo private until the first app release. Added the lens fallback: a ⬇ that gets 401/403/404/410 hides ⬇ and shows the install command (checked in the release app: the real 404 from the private repo, then the message with the command, no ⬇).
- CI: on the owner's request, the Windows and macOS `test` jobs run only on a manual dispatch (`workflow_dispatch`, input `os`); pushes and PRs run Linux only. The models release workflow still runs on `windows-latest` when a `models-opus-mt-*` tag is pushed.
- Next: the owner's answer on the two M1-OCR-6 calls; then "Next up".

### 2026-11-01 — Session 8 (M1-TR-6: CTranslate2 + OPUS-MT)
- Commits: `7c0bcb5` import-only manifest entries + `attribution` + `import-models`; `100849e` `scripts/models/opus_mt.py` (pinned, hash-locked converter, pinned input/output SHA-256); `ac60851` crate `pg-ct2` + manifest entries + `.cargo/config.toml`; `98fdba0` app on `pg-ct2`, lens explains the import; `c32f4e6` CI CMake/C++/Ninja + MSRV job; `84f9dad` removal of `pg-bergamot`, `fxtranslate` and the Mozilla entries; then docs + this checklist.
- Models: newest official pair-specific OPUS-MT releases, `swe-eng/opus+bt-2021-04-30` and `eng-swe/opus+bt-2021-04-14` (transformer-align, 286 MB zips) → CTranslate2 int8, 84.8 MB per direction. Conversion ≈ 20 s; two fresh runs gave byte-identical outputs. Import-only until D-9.
- Verified (Windows): fmt, clippy `-D warnings`, 157 tests (7 ignored), `cargo deny check` (all ok); real-model tests (`cargo test -p pg-ct2 --release --test real_models -- --ignored --test-threads 1`): 5/5 pass. `{n}` placeholders survive OPUS-MT (20/20), SV-05 gate 20/20 (5 masked retries, 0 fallbacks). Latency/RSS in M1-TR-6 and ADR-0005. `npm run tauri dev` from PowerShell (only CMake + Ninja added to `PATH`) built and started the app; the release app translated real Swedish under the lens (117 ms uncached cycle). `cargo +1.90 check --workspace --all-targets` passes.
- Not verified: macOS/Linux builds; `tauri build` bundling with the static CRT; CI (never run).
- Gotchas:
  - **cmake-rs + Visual Studio generator = unoptimized C++.** Without `CMAKE_GENERATOR`, cmake-rs picks the VS generator and overwrites `CMAKE_CXX_FLAGS_RELEASE` with cc's flags, stripping every `/O…`: CTranslate2 ran 341 ms per short string instead of 28.6 ms. Setting `CXXFLAGS_<target>` doesn't help (stripped too); `_CL_=/O2` breaks CMake's Debug compiler probe (`/RTC1`). Ninja works, and needs no `vcvars64` because cmake-rs passes cc's MSVC environment to CMake. Changing the generator needs `cargo clean -p ct2rs -p sentencepiece-sys`.
  - ct2rs links CTranslate2 with `/MT`; without `+crt-static` the link fails with LNK2038. `RUSTFLAGS` in the environment (CI had `-D warnings`) replaces `.cargo/config.toml`'s rustflags.
  - The sentencepiece long-path try-compile failure (session 4) didn't occur at `E:\Dev\Polyglass\target`.
  - From this environment's PowerShell, `npm run tauri dev` still needs the toolchain `bin` first on `PATH` (README troubleshooting, session 2).
  - A `git add` of an already `git rm`'d path fails and leaves the deletion staged for the next commit; check `git show --stat` after each commit.
- Left on this machine for the coordinator to delete by hand: the session 4 Mozilla models in the app's store (`%LOCALAPPDATA%\org.polyglass.desktop\models\bergamot-sv-en`, `bergamot-en-sv`, ≈ 30 MB, ignored by the app now) and the `pg-bergamot` real-model test store under `target/` (gone with `cargo clean`). The converted OPUS-MT files are in `target/opus-mt/` (downloads 572 MB, venv, `out/`); `python scripts/models/opus_mt.py --clean` or deleting the folder frees it.
- Next: see "Next up"; owner decision D-9 for hosting.

### 2026-11-01 — Session 7 (owner decisions D-1..D-5, coordinator)
- The owner answered D-1..D-5. D-1, D-2, D-3 and D-5 are decided and D-4 is deferred (details under "Owner decisions"). SPEC is now v0.3: §3, §4, MODE-02, TR-06, MDL-07 (new), SV-01, SV-10, §8 and §10. PLAN §8 and §11 are updated too.
- D-2 is done: `LICENSE` (Apache-2.0; matches the text that `reqwest` ships, apart from the copyright line), workspace `license`, `package.json` + lock and the README.
- New tasks: M3-8 (configurable language pairs, MDL-07) and M3-9 (German → English). M2-2, M3-2 and M3-4 carry the D-5 details.
- New open question D-8 (`fxtranslate` vs Bergamot C++), raised by the owner.
- README: "Build and run locally" section (`5c50fc7`). The release build (`npm run tauri build -- --no-bundle`) and a `--demo` launch of `target/release/polyglass.exe` were checked; the installer bundle (WiX/NSIS) was not.
- D-8 decided: CTranslate2 + OPUS-MT. ADR-0005 written and ADR-0004 marked superseded. SPEC (TR-01, SIZE-05 → 200 MB, §3), PLAN §4/§8/risks and the README status are updated. M1-TR-6 added, and M1-TR-1..4 are back to `[~]`. D-9 (model hosting) is open.
- Next: M1-TR-6 ("Next up" 1).

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
