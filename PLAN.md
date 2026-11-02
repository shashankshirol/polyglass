# Polyglass — Project Plan

> **Working name:** Polyglass (placeholder; check name/trademark availability before the public launch).
> **Status:** Draft v0.2 · 2026-10-30 · Companion document: [`SPEC.md`](./SPEC.md)

## 1. Summary

Polyglass is a lightweight, open-source desktop app. When launched, it shows a movable, always-on-top **lens**. Text visible beneath the lens is recognized (OCR) and translated in near real time, and the translation is drawn inside the lens. Translation runs **on-device by default**; online backends (LibreTranslate, DeepL, Google) are opt-in.

Built with **Rust + Tauri 2**, targeting **Windows, macOS and Linux**.

## 2. Goals, non-goals, principles

**Goals**
- Move a lens over any on-screen text and read it in your language within about half a second of the lens coming to rest.
- Work fully offline with on-device OCR and translation.
- Stay small: small installer, low idle CPU and memory, no bundled heavyweight runtimes.
- Be transparent: open source, no telemetry, a plain-language data-flow doc, and a visible on-device/cloud indicator.

**Non-goals (v1)**
- Video-rate translation of moving content (subtitles in a playing video).
- Handwriting, vertical CJK text, speech output.
- Mobile platforms, app-store distribution.

**Principles**
1. **Private by default.** Nothing leaves the device unless the user turns on an online backend, and then only recognized text, never screen images.
2. **Use what the OS already ships.** Native OCR on Windows and macOS means no OCR model downloads there.
3. **Do no work when nothing changed.** Capture only the lens region, skip OCR on unchanged frames, cache translations.
4. **Headless core.** All capture/OCR/translation logic lives in plain Rust crates with no Tauri dependency, so it's testable and benchmarkable from a CLI.

## 3. Architecture

```
 ┌────────────────────────── Tauri app (apps/desktop) ───────────────────────────┐
 │                                                                               │
 │  Lens window (WebView: TS + Vite, no framework)    Settings / Model manager   │
 │   ▲ renders LensUpdate events                       window (WebView)          │
 │   │                                                                           │
 │   │ tauri events / commands (typed, validated)                                │
 │ ┌─┴───────────────────────── pg-pipeline ─────────────────────────────────┐   │
 │ │  Scheduler ─► Capture ─► Change detect ─► OCR ─► Block group ─► Cache ─►│   │
 │ │  (lens pos,    (region     (downsampled     │                 │   Translate│
 │ │   rest timer)   only)       hash diff)      │                 │   (backend)│
 │ └─────────────────────────────────────────────┼─────────────────┼──────────┘   │
 │      pg-capture            pg-ocr ────────────┘    pg-translate ┘              │
 │  WGC / ScreenCaptureKit /   WinRT OCR / Vision /   CTranslate2 (on-device) /  │
 │  X11 / xdg-portal           Tesseract              LibreTranslate/DeepL/Google│
 └───────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Repository layout (Cargo workspace)

```
polyglass/
├─ apps/desktop/          # Tauri shell (src-tauri/) + ui/ (TypeScript, Vite)
├─ crates/
│  ├─ pg-core/            # Shared types: PhysRect, Frame, Line, Block, LangPair, errors
│  ├─ pg-capture/         # ScreenCapturer trait + per-OS implementations
│  ├─ pg-ocr/             # OcrEngine trait + WinRT / Vision / Tesseract
│  ├─ pg-translate/       # Translator trait + LibreTranslate / DeepL / Google (on-device CTranslate2 backend in its own crate)
│  ├─ pg-pipeline/        # Scheduler, change detection, block grouping, LRU cache
│  ├─ pg-models/          # Model manifest, download, SHA-256 verify, storage
│  └─ pg-bench/           # CLI: run pipeline on fixtures, print latency percentiles
├─ models/manifest.toml   # Pinned model URLs + SHA-256 + license, reviewed via PR
├─ tests/fixtures/        # Screenshots with known ground-truth text
└─ docs/                  # ARCHITECTURE.md, PRIVACY.md, adr/
```

### 3.2 Core interfaces (sketch)

```rust
pub trait ScreenCapturer: Send {
    /// Capture `region` (physical pixels), excluding Polyglass's own windows where the OS allows.
    fn capture(&mut self, region: PhysRect) -> Result<Frame>;
    fn excludes_self(&self) -> bool;
}

pub trait OcrEngine: Send + Sync {
    fn recognize(&self, frame: &Frame, lang: Option<&LangTag>) -> Result<Vec<Line>>;
    fn supported_languages(&self) -> Vec<LangTag>;
}

#[async_trait]
pub trait Translator: Send + Sync {
    fn id(&self) -> &'static str;
    /// true = never touches the network. Drives the privacy indicator.
    fn is_local(&self) -> bool;
    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>>;
}
```

### 3.3 Runtime model

- The lens UI is a thin renderer. It receives `LensUpdate { blocks: [{ rect, source, translation, bg, fg }] }` events and draws them. No business logic in JS.
- The pipeline runs in Rust. Capture and OCR run on a dedicated worker thread; translation runs on Tauri's async runtime (on-device inference on a blocking pool).
- A new lens position cancels any in-flight work for the old position (generation counter), so stale results are never drawn.
- When the lens is hidden, the pipeline is fully parked: no timers, no capture.

## 4. Key technical decisions (tentative until M0 spikes produce ADRs)

| Area | Decision | Alternatives considered | Why |
|---|---|---|---|
| Shell | Tauri 2 | Electron, egui, Qt | Uses the system webview, small binaries, Rust core. |
| UI | TypeScript + Vite, no framework | Svelte, React | The lens UI is tiny; fewer deps is lighter and easier to audit. |
| Capture (Windows) | Windows.Graphics.Capture + `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` | DXGI Desktop Duplication, GDI | Excludes our own window from capture natively (Win10 2004+). |
| Capture (macOS) | ScreenCaptureKit stream with `sourceRect` = lens, filter excluding our app | CGWindowList APIs (deprecated) | Native self-exclusion and region-only capture. |
| Capture (Linux X11) | x11rb + MIT-SHM | XComposite per-window | Simple and fast; self-exclusion handled by mode design (§5). |
| Capture (Wayland) | xdg-desktop-portal Screenshot → Freeze mode | Portal ScreenCast via PipeWire | Only sanctioned path; live mode is a post-v1 stretch. |
| OCR | WinRT `Windows.Media.Ocr` / Apple Vision / Tesseract 5 on Linux; **Tesseract `swe` for Swedish on macOS** | PaddleOCR via ONNX, `ocrs` | Built into the OS where possible; Apple Vision doesn't support Swedish, so macOS bundles Tesseract for it (SPEC §3, OCR-01). |
| On-device translation | **CTranslate2** (OpenNMT) via `ct2rs`, with OPUS-MT models converted to int8 | The pure-Rust engine and models of ADR-0004 (superseded) | Actively maintained and widely deployed runtime; OPUS-MT has per-pair models for well over a thousand pairs (MDL-07). ≈ 80 MB per direction; 27 ms short / 199 ms paragraph measured. Decided in ADR-0005 (D-8). |
| Online translation | LibreTranslate (self-hosted), DeepL API, Google Cloud Translation | — | As requested; all via `reqwest` with rustls. |
| Secrets | OS keychain via `keyring` crate | Encrypted config file | Never store API keys in plain files. |
| Settings | TOML in platform config dir | JSON, SQLite | Human-readable and diffable. |
| Global hotkeys | `tauri-plugin-global-shortcut` | — | Cross-platform (X11/Win/mac); Wayland noted in §5. |

## 5. Platform notes (the hard parts)

**Capturing what's behind your own window.** The lens draws translated text over the very pixels it needs to read. If the capture includes the lens, OCR would read our own output.
- Windows and macOS: solved by OS-level capture exclusion (above). Overlay mode is Tier 1 here.
- Linux X11: no exclusion API. Options are (a) **Panel mode** (lens interior stays transparent; translation is shown in a strip attached to the lens), which captures cleanly, and (b) Overlay mode that hides the overlay for one frame while capturing, only when the lens comes to rest. (b) causes a brief flicker, so it ships as experimental.
- **Freeze mode** sidesteps the problem everywhere: take one snapshot of the monitor, show it fullscreen, and move the lens over the snapshot.

**Wayland.** Apps can't freely capture the screen, place windows globally, or reliably stay on top (GNOME has no layer-shell). v1 supports Wayland through Freeze mode via the portal (user grants screenshot permission). Global hotkeys depend on the portal GlobalShortcuts interface, which isn't available on every desktop; a tray-menu fallback is required.

**Transparent windows on macOS** need Tauri's `macOSPrivateApi` flag. That rules out the Mac App Store, which is acceptable for an open-source project distributed via GitHub/Homebrew.

**macOS permissions.** Screen Recording permission is required. Recent macOS versions periodically re-confirm this permission for third-party apps; the app must handle revocation gracefully (see SPEC CAP-05).

**Click-through interior.** Users need to scroll and click the content under the lens while keeping the frame draggable. Tauri's `set_ignore_cursor_events` is whole-window, so the app toggles it based on cursor hit-testing (polled at about 30 Hz, only while the lens is visible). Validate in M0 that this is smooth on all OSes.

**Short UI strings.** Translating isolated labels lacks context and can read oddly. Mitigation: translate per block (paragraph/label group), not per line, and keep "show original" one keypress away.

## 6. Latency budget (lens at rest → translation drawn, REF-A)

| Stage | Budget p50 (see SPEC §5) |
|---|---|
| Rest debounce | 150 ms |
| Capture region | ≤ 20 ms |
| Change detection | ≤ 2 ms |
| OCR (600×300 px, ~40 words) | ≤ 200 ms |
| Block grouping | ≤ 5 ms |
| Translation, on-device, uncached | ≤ 250 ms |
| Render | ≤ 16 ms |
| **Total** | **≈ 600 ms** → SPEC PERF-01 (measured on Swedish → English) |

## 7. Roadmap

Rough sizing assumes one or two part-time contributors. Each milestone ends with its SPEC acceptance criteria passing (traceability table in SPEC §9).

**M0 — Spikes and foundations (≈ 3 weeks).** Public repo from day one with this plan, the spec, license, and CONTRIBUTING.
- Spike 1: transparent, frameless, always-on-top, click-through Tauri window on Win/mac/Linux.
- Spike 2: self-excluding region capture on Windows and macOS; X11 hide-capture-show flicker measurement.
- Spike 3: on-device translation engines built from Rust on all three OSes, using the sv↔en models. Compare build complexity, binary size, latency, memory. Result: ADR-0004 (Windows), then ADR-0005 chose CTranslate2 + OPUS-MT.
- Spike 4: create the Swedish fixtures; measure OCR latency and accuracy per engine, including Vision vs Tesseract `swe` for Swedish on macOS.
- Spike 5: Wayland portal screenshot + fullscreen Freeze window on GNOME and KDE.
- Output: ADRs in `docs/adr/`, CI skeleton, `pg-bench` harness.

**M1 — MVP on Windows + macOS (≈ 5 weeks).** Overlay mode, on-device only, model manager with download + verify, Swedish ↔ English (SPEC §3), tray icon, basic settings. Private alpha builds from CI.

**M2 — Linux + more modes (≈ 4 weeks).** X11 capture, Tesseract, Panel mode, Freeze mode on all platforms, Wayland via Freeze mode. Public alpha.

**M3 — Online backends (≈ 3 weeks).** LibreTranslate, DeepL, Google; consent dialogs; keychain storage; network activity log; persistent on-device/cloud indicator.

**M4 — Polish (≈ 3 weeks).** Configurable hotkeys, accessibility pass, Swedish UI translation, multi-monitor/mixed-DPI hardening, UI string externalization. Beta.

**M5 — 1.0 hardening (≈ 3 weeks).** Code signing and notarization, opt-in signed updater, performance gate in CI, docs site, packaging (MSI/NSIS, DMG, .deb, .rpm, AppImage, Flatpak if feasible, Homebrew cask, winget).

## 8. Open-source and release setup

**Licensing.** **Apache-2.0** (owner decision D-2, 2026-11-01; see `LICENSE`). Third-party: CTranslate2 is MIT, OPUS-MT models are CC-BY-4.0 (attribution, shown in the model manager), Tesseract is Apache-2.0. Translation models carry their own licenses and are downloaded at runtime, not bundled; each model's license is shown in the model manager and recorded in `models/manifest.toml`. Generate `THIRD_PARTY_NOTICES` with `cargo-about`.

**Repository files.** README (quick start under 5 minutes), CONTRIBUTING, CODE_OF_CONDUCT (Contributor Covenant), SECURITY.md (private disclosure channel), GOVERNANCE.md (lightweight maintainer model), **PRIVACY.md** (plain-language data flow, linked from the app), DCO sign-off on commits (no CLA).

**CI (GitHub Actions)** on Windows, macOS (arm64; Intel Macs are out of v1, D-11), Ubuntu:
- `cargo fmt --check`, `cargo clippy -D warnings`, unit and fixture tests.
- `cargo deny` (licenses, advisories, banned crates), `npm audit` for the UI.
- `pg-bench` on fixtures, failing the build on >20% latency regression vs main.

**Releases.** Built only in CI from signed tags. Publish `SHA256SUMS`, a CycloneDX SBOM, and build-provenance attestations so anyone can verify that binaries match the source. Code signing: Windows via SignPath Foundation (free for OSS), macOS Developer ID + notarization (requires a paid Apple Developer account). Unsigned builds trigger SmartScreen/Gatekeeper warnings, which matters a lot for an app that captures the screen.

## 9. Testing strategy

- **Unit tests** for block grouping, change detection, cache, manifest verification.
- **Fixture tests**: screenshots with ground-truth text at 100/150/200% scale; measure OCR character error rate per engine (SPEC OCR-04).
- **Pipeline tests** with mock capturer/translator to check cancellation, staleness, caching.
- **Self-capture test**: lens shows a known marker string; assert OCR output never contains it (SPEC CAP-02).
- **Offline test**: run the full app with networking disabled (Linux `unshare -n`, firewall rule on Win/mac) and assert it works (SPEC PRIV-01).
- **Manual QA checklist** per OS for window behavior, permissions, multi-monitor.

## 10. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| CTranslate2 (C++, CMake) is hard to build/link on all OSes (esp. Windows, arm64) | Delays on-device mode | Built on Windows in the ADR-0004 spike; CI on all OSes; the engine stays behind the `Translator` trait |
| Wayland limits live capture and always-on-top | Linux UX weaker | Freeze mode in v1; live ScreenCast as stretch goal; be upfront in README |
| macOS permission re-prompts annoy users | Churn | Clear onboarding, graceful revoke handling, docs |
| Unsigned binaries flagged by AV/SmartScreen (screen capture + hotkeys look suspicious) | Trust, adoption | Sign from first public beta; reproducible-ish builds; provenance |
| Click-through toggling feels laggy | Core UX | M0 spike; fall back to "hold modifier to interact with content below" |
| Poor translations of short UI fragments | Perceived quality | Block-level translation, show-original key, multiple backends |
| Swedish OCR on macOS depends on bundled Tesseract (larger app, possibly weaker on small UI text than Vision) | å/ä/ö errors, size | SPEC SV-03 accuracy gate; upscale small text before OCR; switch to Vision automatically if Apple adds Swedish (OCR-06) |
| OCR weak on stylized/low-contrast text | Missed text | Upscale small regions before OCR, confidence threshold, Freeze mode for zooming |
| Model licensing ambiguity | Legal | Per-model license in manifest, reviewed in PRs; no bundling |

## 11. Decisions needed from the project owner

Answered by the owner on 2026-11-01 (tracked as D-1..D-5 in `CHECKLIST.md`):

1. ~~Final project name~~ **Polyglass** (D-1).
2. ~~License choice~~ **Apache-2.0** (D-2).
3. ~~Language pairs for v1~~ Swedish ↔ English is primary (SPEC §3); the SPEC §10 language questions are answered there (D-5).
4. ~~Minimum OS versions~~ **Windows 10 2004, macOS 13, Ubuntu 22.04** as proposed; no support for older systems (D-3).
5. Budget for code signing (Apple Developer Program fee; SignPath application for Windows): **deferred** until before the first public beta (D-4).
