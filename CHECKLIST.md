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

- **Milestone:** M0 (foundations). Headless core done: 98 tests green, clippy `-D warnings` clean, fmt clean (Windows only so far). Platform spikes not started.
- **Dev machine used so far:** Windows 11 x64, Rust 1.99, Node 24, Python 3.12 (no Pillow), no Tesseract. Windows OCR languages installed: `en-US`, `en-GB` (no `sv-SE`, which is useful for testing SV-02).
- **Git:** `git init` done locally on `main`; **nothing committed yet**, no remote.
- **Next up (suggested order, each ≈ one session):**
  1. M0-SPK-1 + M1-APP-1: Tauri 2 scaffold in `apps/desktop` with a transparent click-through lens window.
  2. M1-OCR-1/2: Windows OCR engine (`windows` crate) in `pg-ocr`, testable on this machine.
  3. M1-CAP-1/2: WGC region capture + `WDA_EXCLUDEFROMCAPTURE`.
  4. M0-FIX-2: fixture image generator (unblocks OCR accuracy gates and real `pg-bench` runs).
  5. M0-SPK-3: Bergamot vs CTranslate2.
- **Known follow-ups inside done work** (not separate tasks yet):
  - `ChangeConfig` thresholds (8 px cells, 6-level tolerance, 0.15% of cells) are only tested on synthetic frames. Tune on real captures (caret blink, ClearType).
  - `langid` scores 100% on `mixed-sv-en`, but it was tuned on that same fixture. Add a held-out set before trusting the SV-04 ≥ 90% gate.
  - `reqwest` uses rustls with the default **aws-lc-rs** provider. It built fine here, but check its binary size (SIZE-01) and build needs on macOS/Linux/arm64; switch to `ring` if needed.
  - An OCR call already running when the lens moves runs to completion and its result is discarded. OCR never *starts* while moving (ADR-0001).
  - The pipeline queries OCR languages through `recognize` errors on each cycle. Cache the language list if WinRT makes that slow.

## Owner decisions pending (PLAN §11, SPEC §10)

- [ ] D-1 Final project name (crate prefix `pg-` assumes "Polyglass").
- [ ] D-2 License. Workspace currently declares `MIT OR Apache-2.0` (PLAN recommendation); LICENSE files not yet added.
- [ ] D-3 Minimum OS versions confirmed (proposed: Win10 2004, macOS 13, Ubuntu 22.04).
- [ ] D-4 Code-signing budget (Apple Developer Program, SignPath).
- [ ] D-5 SPEC §10 open questions 1–6 (default direction, pivot languages, UK/US English default, Linux default mode, online→on-device fallback, Apple Translation framework).

---

## M0 — Spikes and foundations

### Repository and tooling
- [x] M0-REPO-1 Cargo workspace with `crates/pg-{core,capture,ocr,translate,pipeline,models,bench}`.
- [x] M0-REPO-2 `.gitignore`, `rustfmt.toml`, workspace lints, `deny.toml`.
- [~] M0-REPO-3 CI skeleton `.github/workflows/ci.yml`: fmt, clippy `-D warnings`, tests on Windows/macOS (arm64 + x86_64)/Ubuntu, `pg-bench` smoke run, `cargo deny` (QUAL-01, SEC-05 skeleton). *Written but never run: no remote yet. `deny.toml` not run locally (cargo-deny not installed).*
- [x] M0-REPO-4 Docs skeleton: README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, GOVERNANCE, PRIVACY, docs/ARCHITECTURE.md, docs/adr/0000-template + ADR-0001 (OSS-03 skeleton). *Contact e-mails are `.invalid` placeholders.*
- [ ] M0-REPO-5 Add LICENSE-MIT / LICENSE-APACHE once D-2 is decided.
- [~] M0-REPO-6 Repo history + public remote. *Done: local `git init`. Not done: first commit, remote, real contact addresses.*
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
- [ ] M0-SPK-1 Transparent, frameless, always-on-top Tauri 2 window; click-through via cursor hit-testing at ~30 Hz (PLAN §5). Measure smoothness on Win/mac/Linux. → ADR-0002.
- [ ] M0-SPK-2 Self-excluding region capture: WGC + `WDA_EXCLUDEFROMCAPTURE` (Win), ScreenCaptureKit (mac); X11 hide/capture/show flicker timing. → ADR-0003.
- [ ] M0-SPK-3 Bergamot (cxx FFI) vs CTranslate2 (`ct2rs`) with sv↔en models: build complexity, size, latency, memory on all OSes. → ADR-0004.
- [ ] M0-SPK-4 Swedish fixtures + OCR accuracy/latency per engine incl. Vision vs Tesseract `swe` on macOS. → ADR-0005.
- [ ] M0-SPK-5 Wayland portal screenshot + fullscreen Freeze window on GNOME and KDE. → ADR-0006.

### Fixtures (SPEC §5, CC0)
- [~] M0-FIX-1 Ground-truth text for `ui-dialog-sv`, `ui-dialog-en`, `mixed-sv-en`, `numbers-sv`, `numbers-en`, `paragraphs-sv`, `paragraphs-en`, `long-compounds-en` in `tests/fixtures/*/truth.json` (format in `tests/fixtures/README.md`). *Done: text, block lists, expected languages. Not done: `bbox` per block, images, a held-out `mixed-sv-en-2` for langid, and a native-speaker review of the Swedish.*
- [ ] M0-FIX-2 Fixture image generator (render truth.json to PNG at 100/125/150/200%, light + dark theme, 12–16 px UI fonts). Python+Pillow isn't installed here; consider a Rust generator (`ab_glyph`/`cosmic-text`) or a headless-browser script.
- [ ] M0-FIX-3 CER scorer (per-char + å/ä/ö-specific error report, SV-03) in `pg-bench`.
- [ ] M0-FIX-4 Eval sets `tests/eval/sv-en`, `tests/eval/en-sv`, 200 segments each with references (SV-07).

---

## M1 — MVP on Windows + macOS

### Capture
- [ ] M1-CAP-1 Windows: Windows.Graphics.Capture region capture (CAP-01) in `pg-capture::windows`.
- [ ] M1-CAP-2 Windows: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on lens window; marker self-capture test over 100 cycles (CAP-02).
- [ ] M1-CAP-3 macOS: ScreenCaptureKit stream, `sourceRect` = lens, excluding own app (CAP-01/02).
- [ ] M1-CAP-4 macOS Screen Recording permission missing/revoked → explanation + open-settings button + auto-resume (CAP-05).
- [ ] M1-CAP-5 HiDPI/mixed-DPI fixture tests at 100/125/150/200% (CAP-04, LENS-02).

### OCR
- [ ] M1-OCR-1 Windows OCR engine via `windows` crate (`Windows.Media.Ocr`), lines + boxes + confidence (OCR-01/02).
- [ ] M1-OCR-2 Runtime language query `AvailableRecognizerLanguages` (OCR-06) + SV-02 Windows message/link when `sv-SE` missing.
- [ ] M1-OCR-3 Apple Vision engine (English) + runtime language check (OCR-01, OCR-06).
- [ ] M1-OCR-4 Tesseract 5 engine (bundled on macOS with `swe`+`eng`) (OCR-01, SV-02 mac).
- [ ] M1-OCR-5 Upscale small text before OCR (PLAN §10 risk mitigation).
- [ ] M1-OCR-6 Accuracy gates on fixtures: OCR-04 (en), SV-03 (sv, å/ä/ö).

### Translation and models
- [ ] M1-TR-1 On-device backend per ADR-0004 implementing `Translator`, `is_local() == true` (TR-01).
- [ ] M1-TR-2 Model manifest entries for sv→en, en→sv with real URLs/SHA-256/licenses (MDL-02, SIZE-05).
- [ ] M1-TR-3 Both directions loaded → instant swap (SV-01).
- [ ] M1-TR-4 Preserved-token gate on `numbers-sv`/`numbers-en` with the real backend (SV-05).

### Desktop app (`apps/desktop`)
- [ ] M1-APP-1 Tauri 2 scaffold, TS + Vite UI, strict CSP, least-privilege capabilities (SEC-01, SEC-02).
- [ ] M1-APP-2 Lens window: launch/center/restore (LENS-01), drag + arrow keys (LENS-02), resize limits (LENS-03), click-through (LENS-04).
- [ ] M1-APP-3 Toolbar with accessible controls + privacy indicator (LENS-05, PRIV-05).
- [ ] M1-APP-4 Tray icon, no taskbar/Dock entry, show/hide hotkey parks the pipeline (LENS-01, LENS-06).
- [ ] M1-APP-5 Wire `pg-pipeline` driver to lens events; render `LensUpdate` in Overlay mode (MODE-01, REN-01).
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
- [ ] M2-2 Panel mode (MODE-02) incl. flip-above and reading order.
- [ ] M2-3 Freeze mode on all platforms; Wayland via xdg-desktop-portal (MODE-03, CAP-05 Wayland).
- [ ] M2-4 Tray-menu fallback for hotkeys on Wayland (HK-03).
- [ ] M2-5 Low-confidence hide/dim setting (OCR-05) — pipeline side already filters by threshold; UI setting pending.
- [ ] M2-6 "No text found" for blank regions in UI (CAP-06) — detection done in pipeline.
- [ ] M2-7 Linux packaging deps on Swedish Tesseract data; AppImage bundles it (SV-02 Linux, SIZE-03/04).

## M3 — Online backends
- [ ] M3-1 LibreTranslate backend + language-list check for Swedish (TR-01, SV-10).
- [ ] M3-2 DeepL Free/Pro + EN-GB/EN-US variant (TR-01, SV-10).
- [ ] M3-3 Google Cloud Translation (TR-01).
- [ ] M3-4 Batching per frame (TR-05); 5 s timeouts and error mapping (TR-06) — `TranslateError` kinds already exist.
- [ ] M3-5 Keychain storage via `keyring` (SET-02) + grep test.
- [ ] M3-6 Consent dialogs per backend (PRIV-04); text-only payloads test (PRIV-03).
- [ ] M3-7 Network activity log (PRIV-08); rustls only (SEC-03).

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

### 2026-10-31 — Session 1
- Created this checklist. Scaffolded the workspace (edition 2024, resolver 3, workspace lints incl. `unsafe_code = "deny"`; platform crates will need a module-level `allow`), docs, ADR-0001, CI and `deny.toml`.
- Implemented M0-CORE-1..7. `cargo test --workspace`: 98 passed, and the timing-dependent driver tests passed 8 runs in a row (they use Tokio's paused clock). Clippy and fmt are clean.
- Fixture ground truth written for all 8 SPEC fixtures (text only).
- Not done or not verified: CI never ran (no remote), `cargo deny` never ran, nothing committed, nothing tested on macOS/Linux.
- Gotchas:
  - Windows OCR here has no `sv-SE`; Tesseract and Pillow aren't installed.
  - `tokio::select!` evaluates disabled branches' futures, so deadlines use `unwrap_or_else(Instant::now)`, not `unwrap()`.
  - `CycleOutcome::Update` holds a `Box<LensUpdate>` (clippy `large_enum_variant`).
  - SV-05 value checks compare only fractional/dotted numbers, because Swedish phone numbers (`08-123 456 78`) look like thousands grouping.
- Next: see "Next up" above.
