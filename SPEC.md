# Polyglass — Product Specification

> **Version:** 0.3 (draft) · **Date:** 2026-11-01 · **Companion:** [`PLAN.md`](./PLAN.md)
> Priorities use MoSCoW: **MUST** (blocks 1.0), **SHOULD** (expected in 1.0), **COULD** (nice to have).
> Each requirement has an ID; acceptance criteria (AC) are written so they can be checked by a test or a reviewer.

**Changes in v0.3:** Owner answers to the open questions (§10, decision D-5). Default direction is Swedish → English (SV-01). Language pairs come from the model catalog, so users can add their own (MDL-07); German → English is the next pair (§3). US English is the default variant (SV-10). Panel is the default mode on Linux X11 (MODE-02). Failing online backends fall back to on-device with a notice (TR-06). Apple's Translation framework is out of scope (§8). Minimum OS versions confirmed (§4, D-3).

**Changes in v0.2:** Swedish is the primary v1 language. Added §3 (language scope) and §6.11 (Swedish requirements SV-01 to SV-10). OCR engine is now chosen per platform *and* source language, because Apple Vision has no Swedish support. Fixtures, performance measurement and UI localization now cover Swedish. macOS installer size target raised to 20 MB for the bundled Swedish OCR.

## 1. Purpose

Polyglass shows a movable lens over the desktop. Text beneath the lens is recognized and translated in near real time, and the translation is shown in the lens. All processing can run on-device.

## 2. Terms

| Term | Meaning |
|---|---|
| Lens | Frameless, always-on-top, transparent window the user moves over text. |
| Region | Screen pixels beneath the lens interior. |
| Frame | One captured image of the region. |
| Line / Block | OCR output line; a block is a group of lines translated as one unit (paragraph, label). |
| Engine | OCR implementation. |
| Backend | Translation implementation (on-device or online). |
| On-device mode | All OCR and translation run locally; no network traffic except user-started model downloads and opt-in update checks. |
| At rest | The lens has not moved for ≥ 150 ms. |

## 3. Language scope (v1)

Swedish is the primary language for v1.

| Priority | Direction | Notes |
|---|---|---|
| MUST | Swedish → English | On-device and online |
| MUST | English → Swedish | On-device and online |
| SHOULD | German → English | Next pair after sv↔en (D-5): most users are English speakers. On-device model from the same catalog |
| SHOULD | Swedish ↔ other languages that have on-device models | Pivot through English (TR-03) |
| COULD | Any other pair a backend offers | No Swedish-specific testing |

Language pairs are data, not code: any pair the model catalog or an online backend offers can be installed and selected without a code change (MDL-07).

**Platform facts that shape this spec** (re-verify in M0, since vendor support changes):

- **On-device translation:** Swedish is one of the languages supported by Firefox Translations, whose Bergamot models Polyglass uses, so sv↔en works fully offline.
- **OCR on Windows:** Windows OCR supports Swedish (`sv-SE`), but only once the Swedish OCR feature is installed on the machine.
- **OCR on macOS:** Apple Vision does not support Swedish text recognition. Swedish source text on macOS therefore uses Tesseract with Swedish data by default (OCR-01, SV-02).
- **OCR on Linux:** Tesseract with Swedish data (`tesseract-ocr-swe` on Debian/Ubuntu, `tesseract-langpack-swe` on Fedora).

**Swedish text features the tests must cover:**

- Letters å ä ö / Å Ä Ö, and é in loanwords.
- Long compound words, which affect line breaking and fitting.
- Swedish number and date formats: `1 234,50 kr`, `12,5 %`, `30 okt. 2026`, `2026-10-30`.
- Mixed Swedish/English text, which is common in Swedish software UIs and tech content.

## 4. Platform support (v1)

| Platform | Minimum | Tier | Overlay mode | Panel mode | Freeze mode |
|---|---|---|---|---|---|
| Windows x64 | Windows 10 2004 | 1 | ✅ | ✅ | ✅ |
| Windows arm64 | Windows 11 | 2 | ✅ | ✅ | ✅ |
| macOS (Apple silicon, Intel) | macOS 13 | 1 | ✅ | ✅ | ✅ |
| Linux X11 | Ubuntu 22.04 / Fedora 39 | 2 | Experimental | ✅ | ✅ |
| Linux Wayland | GNOME 45 / KDE Plasma 6 | 2 | ❌ (post-v1) | ❌ (post-v1) | ✅ |

Tier 1: release-blocking. Tier 2: supported, but bugs don't block a release.

The minimums are confirmed (D-3). Older OS versions are not supported.

## 5. Reference hardware and measurement

- **REF-A:** x86_64, 4 cores / 8 threads (Intel Core i5-10210U class), 8 GB RAM, integrated GPU; Windows 11 and Ubuntu 24.04.
- **REF-B:** Apple M1, 8 GB RAM, macOS 14.
- **Method:** `pg-bench` runs the pipeline in a 600×300 logical-px lens at 100% scale on two fixtures: `ui-dialog-sv` (≈ 40 words, Swedish → English; the primary measurement) and `ui-dialog-en` (≈ 40 words, English → Swedish). 5 warm-up runs, then percentiles over 100 runs. In-app timings come from the debug trace (`--trace-timings`).
- **Fixtures** are created by the project and released under CC0, so they can be shared and extended freely.

## 6. Functional requirements

### 6.1 Lens window (LENS)

**LENS-01 Launch — MUST**
- [ ] On first launch the lens appears within 1 s, centered on the monitor containing the cursor, at 480×200 logical px.
- [ ] On later launches, last size and mode are restored; last position is restored if that monitor still exists, otherwise the lens is centered.
- [ ] The lens stays above normal application windows when other apps have focus.
- [ ] The app shows a tray / menu-bar icon and no taskbar/Dock entry for the lens.

**LENS-02 Move — MUST**
- [ ] The lens can be dragged by its frame or handle.
- [ ] With the lens focused, arrow keys move it 1 px (Shift: 10 px).
- [ ] Moving the lens between monitors with different scale factors yields a correct translation within one pipeline cycle after coming to rest.

**LENS-03 Resize — MUST**
- [ ] Resizable from all edges and corners; minimum 120×48 logical px; maximum = monitor work area.

**LENS-04 Click-through interior — MUST**
- [ ] With click-through on (default), clicks and scroll-wheel events inside the lens interior reach the window underneath.
- [ ] Frame, handle and toolbar remain interactive at all times.
- [ ] Click-through can be toggled from the toolbar and by hotkey.

**LENS-05 Toolbar — MUST**
- [ ] Shows: source language, target language, swap, mode, pause/resume, privacy indicator (PRIV-05), settings, close.
- [ ] Every control is reachable by keyboard and has an accessible name.

**LENS-06 Show/hide — MUST**
- [ ] A global hotkey and the tray menu toggle the lens.
- [ ] While hidden, no capture, OCR or translation runs (verified by trace showing zero pipeline cycles).

**LENS-07 Appearance — SHOULD**
- [ ] User can set frame color, overlay opacity (20–100%), font family and font scale (75–200%).
- [ ] Respects the OS reduced-motion setting (no animated transitions).

### 6.2 Modes (MODE)

**MODE-01 Overlay — MUST (Tier 1 platforms)**
- [ ] Each translated block is drawn over the screen position of its source block.

**MODE-02 Panel — MUST**
- [ ] The lens interior stays transparent; translations are listed in a panel attached below the lens, flipping above when there is no room below.
- [ ] Panel lists blocks in reading order of the source.
- [ ] Panel is the default mode on Linux X11; Overlay stays selectable there as experimental (§4).

**MODE-03 Freeze — MUST**
- [ ] A hotkey or toolbar button takes one snapshot of the current monitor and displays it fullscreen; the lens can then be moved over the snapshot.
- [ ] Esc or the same hotkey exits Freeze mode and restores the live desktop.
- [ ] Freeze mode works on Wayland using the xdg-desktop-portal Screenshot interface.
- [ ] The snapshot is held in memory only and discarded on exit (PRIV-02).

### 6.3 Capture (CAP)

**CAP-01 Region-only capture — MUST**
- [ ] In live modes, only the lens interior region (in physical pixels) is captured, not the whole screen.

**CAP-02 Self-exclusion — MUST (Tier 1)**
- [ ] On Windows and macOS, frames never contain Polyglass windows. Test: the lens renders a known marker string over fixture text; OCR output over 100 cycles never contains the marker.
- [ ] On X11 Overlay mode (experimental), the overlay is hidden for at most one frame per capture, and only when at rest or when content changes.

**CAP-03 Cadence — MUST**
- [ ] While the lens is moving, no OCR runs.
- [ ] When at rest, a capture runs immediately; afterwards the region is re-checked for changes at 4 Hz.
- [ ] If the frame differs from the previous one by less than the change threshold, OCR and translation are skipped.
- [ ] Results for a previous lens position are never drawn after the lens has moved (stale-result test).

**CAP-04 HiDPI and multi-monitor — MUST**
- [ ] Correct capture region at 100%, 125%, 150% and 200% scaling, and across monitors with mixed scaling (fixture tests at each scale).

**CAP-05 Permissions — MUST**
- [ ] On macOS, if Screen Recording permission is missing or revoked, the lens shows an explanation and a button that opens the relevant System Settings pane. The app doesn't crash and resumes automatically once permission is granted.
- [ ] On Wayland, a denied portal request shows an explanation and a retry button.

**CAP-06 Blank or protected content — SHOULD**
- [ ] If the region is uniformly black/blank (e.g. DRM-protected video), the lens shows "No text found" instead of garbage output.

### 6.4 OCR (OCR)

**OCR-01 Engines — MUST**
- [ ] The default engine depends on platform and source language:

| Platform | English source | Swedish source |
|---|---|---|
| Windows | Windows OCR (`en-US`) | Windows OCR (`sv-SE`) |
| macOS | Apple Vision | Tesseract 5 (`swe`) |
| Linux | Tesseract 5 (`eng`) | Tesseract 5 (`swe`) |

- [ ] On macOS, Vision becomes the Swedish default only if OCR-06 finds Swedish in Vision's supported languages *and* Vision passes SV-03 on the Swedish fixtures.
- [ ] Tesseract is selectable on all platforms where it is available.

**OCR-02 Output — MUST**
- [ ] Engine output is lines with bounding boxes and confidence; lines are grouped into blocks by proximity and alignment.
- [ ] Block grouping on fixtures `paragraphs-sv` and `paragraphs-en` matches ground truth for ≥ 90% of blocks.

**OCR-03 Source language — MUST**
- [ ] User can pick Swedish, English, or "Auto (Swedish/English)" (see SV-04) as the source language.
- [ ] If the OCR data for the selected language is missing, the lens shows an actionable message (Swedish on Windows: see SV-02).

**OCR-04 Accuracy — MUST**
- [ ] On the English fixture set (UI text 12–16 px, scales 100–200%), character error rate ≤ 3% for Windows OCR and Vision, ≤ 5% for Tesseract.
- [ ] Swedish accuracy is specified in SV-03.

**OCR-05 Low confidence — SHOULD**
- [ ] Lines below the confidence threshold (configurable, default 0.5) are hidden or dimmed per user setting.

**OCR-06 Runtime language check — MUST**
- [ ] Each engine's supported/installed languages are queried at runtime (on Windows via `OcrEngine.AvailableRecognizerLanguages`), never hard-coded, so OS updates that add or remove languages are picked up without an app update.

### 6.5 Translation (TR)

**TR-01 Backends — MUST**
- [ ] On-device backend (Bergamot, or CTranslate2 per M0 ADR) with sv→en and en→sv models in the model manifest.
- [ ] LibreTranslate (user-supplied URL, optional API key).
- [ ] DeepL API (Free and Pro endpoints).
- [ ] Google Cloud Translation (API key).
- [ ] All backends implement the same `Translator` trait and are selectable in Settings.

**TR-02 Default — MUST**
- [ ] A fresh install uses the on-device backend. No online backend is enabled without explicit user action.

**TR-03 Pivot — SHOULD**
- [ ] If no direct on-device model exists for a pair but both X→English and English→Y exist (e.g. Swedish → German), translation pivots through English and the toolbar shows "via EN".

**TR-04 Cache — MUST**
- [ ] Translations are cached in an in-memory LRU (default 2,000 entries) keyed by backend, language pair and normalized text.
- [ ] A cached result is drawn within 100 ms of coming to rest (PERF-02).
- [ ] The cache is never written to disk.

**TR-05 Batching — SHOULD**
- [ ] All blocks from one frame are sent to an online backend in a single request where the API supports it.

**TR-06 Failure handling — MUST**
- [ ] Network errors, timeouts (5 s), HTTP 401/403 (auth) and 429 (quota) each show a distinct, human-readable message in the lens.
- [ ] The app never falls back from on-device to an online backend.
- [ ] When an online backend fails, the app falls back to the on-device backend for that pair if its models are installed, and the lens says so ("Online translation failed — using on-device"). This fallback is on by default and can be turned off in settings.

**TR-07 Already in target language — MUST**
- [ ] Blocks detected as already in the target language are shown unchanged and not sent to the backend (see SV-04).

### 6.6 Rendering (REN)

**REN-01 Legibility — MUST**
- [ ] In Overlay mode, each block's background is filled with a color sampled from the source block's surroundings, and text color achieves ≥ 4.5:1 contrast against it.

**REN-02 Fitting — MUST**
- [ ] Font size shrinks to fit the source box, down to 9 px. If the text still overflows, the box grows downward and the full text is available in Panel view or tooltip.

**REN-03 Scripts — COULD**
- [ ] Right-to-left (Arabic, Hebrew) and complex scripts (Devanagari, Thai) render with correct direction and shaping. (Lowered from SHOULD: not needed for Swedish ↔ English.)

**REN-04 Show original — MUST**
- [ ] Holding a modifier key (default Alt / Option) shows the original text in place of the translation.

**REN-05 Copy — MUST**
- [ ] A hotkey and toolbar action copy all translated blocks in reading order to the clipboard, with å ä ö preserved (UTF-8 round-trip test).

**REN-06 Screen readers — SHOULD**
- [ ] New translations are announced to the OS screen reader via a polite live region, rate-limited to one announcement per 2 s, with the block's `lang` attribute set so the screen reader uses a Swedish or English voice as appropriate.

### 6.7 Model management (MDL)

**MDL-01 Catalog — MUST**
- [ ] Model manager lists available language pairs with download size, license and source URL. Swedish pairs are listed first.

**MDL-02 Integrity — MUST**
- [ ] Models download only from URLs in `models/manifest.toml` shipped with the app.
- [ ] Every file is verified against its SHA-256 from the manifest; on mismatch the file is deleted and an error is shown.

**MDL-03 Downloads — MUST**
- [ ] Downloads show progress, can be cancelled, and resume after interruption.

**MDL-04 Offline import — SHOULD**
- [ ] A model package can be imported from a local file and is verified the same way.

**MDL-07 User-selected language pairs — SHOULD**
- [ ] Users can install any pair the catalog lists and pick it as source/target in the toolbar; adding a pair the on-device engine supports needs only a catalog entry, no code change.
- [ ] Pipeline, OCR language selection and language identification take the pair from settings instead of assuming Swedish/English.

**MDL-05 Removal — MUST**
- [ ] Models can be deleted; total disk usage is displayed.

**MDL-06 No bundled translation models — MUST**
- [ ] The installer contains no translation models. The first-run wizard offers the download (SV-01).

### 6.8 Settings (SET)

**SET-01 Storage — MUST**
- [ ] Settings are stored as human-readable TOML in the platform config directory.

**SET-02 Secrets — MUST**
- [ ] API keys are stored only in the OS keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux). A grep of the config dir and logs for the key returns nothing.

**SET-03 Reset — MUST**
- [ ] "Reset to defaults" restores all settings (keys and models are kept unless the user chooses to remove them).

**SET-04 Autostart — COULD**
- [ ] Opt-in launch at login, off by default.

### 6.9 Hotkeys (HK)

**HK-01 Defaults — MUST**

| Action | Windows / Linux | macOS |
|---|---|---|
| Show/hide lens | Ctrl+Alt+L | ⌘⌥L |
| Freeze mode | Ctrl+Alt+F | ⌘⌥F |
| Copy translation | Ctrl+Alt+C | ⌘⌥C |
| Swap languages | Ctrl+Alt+S | ⌘⌥S |
| Toggle click-through | Ctrl+Alt+T | ⌘⌥T |

These keys were chosen because they produce no characters with AltGr on the Swedish keyboard layout (see SV-09).

**HK-02 Configurable — SHOULD**
- [ ] All hotkeys can be rebound; conflicts with already-registered shortcuts and with AltGr characters on the active keyboard layout are reported.

**HK-03 Wayland — MUST**
- [ ] Where global shortcuts are unavailable, every hotkey action is available from the tray menu, and Settings explains why.

### 6.10 Privacy and transparency (PRIV)

**PRIV-01 Offline guarantee — MUST**
- [ ] With models installed and networking disabled at the OS level, all on-device features work, including Swedish OCR on every platform.
- [ ] In on-device mode, a network monitor records no outbound connections except user-started model downloads and opt-in update checks.

**PRIV-02 No image persistence — MUST**
- [ ] Captured frames and Freeze snapshots are never written to disk. The only exception is a developer flag (`--debug-dump-frames`), which shows a persistent red banner while active.

**PRIV-03 Text only — MUST**
- [ ] Online backends receive only recognized text and the language pair, never images.

**PRIV-04 Consent — MUST**
- [ ] First use of each online backend shows a dialog naming the provider, what is sent, and a link to the provider's privacy policy. Consent is stored per backend and can be revoked in Settings.

**PRIV-05 Indicator — MUST**
- [ ] The lens always shows whether the active backend is on-device (lock icon) or online (cloud icon + provider name).

**PRIV-06 No telemetry — MUST**
- [ ] The app contains no analytics, telemetry or automatic crash upload. Crash reports are saved locally for the user to attach to issues manually.

**PRIV-07 Logs — MUST**
- [ ] At default log level, logs contain no recognized or translated text.

**PRIV-08 Network activity log — SHOULD**
- [ ] Settings includes a view listing every outbound request this session (time, host, bytes sent/received), with no content.

### 6.11 Swedish language support (SV)

**SV-01 First-run defaults — MUST**
- [ ] The default direction is Swedish → English, whatever the OS display language (D-5).
- [ ] The first-run wizard pre-selects both the sv→en and en→sv on-device models and shows their combined download size before downloading.
- [ ] The swap hotkey (HK-01) switches direction instantly, without reloading models once both are downloaded.

**SV-02 Swedish OCR availability — MUST**
- [ ] **Windows:** if Swedish is missing from the installed OCR languages (OCR-06), the lens explains how to add Swedish in Windows language settings and links there. Translation of English source keeps working meanwhile.
- [ ] **macOS:** the Tesseract library and its Swedish and English recognition data ship inside the app, so Swedish OCR works offline from first launch.
- [ ] **Linux:** .deb and .rpm packages depend on the distribution's Swedish Tesseract data package; the AppImage bundles it.

**SV-03 Swedish OCR accuracy — MUST**
- [ ] On the Swedish fixture set (UI text 12–16 px, scales 100–200%, light and dark themes), the default Swedish engine on each platform reaches a character error rate ≤ 3%.
- [ ] Error rate on å ä ö Å Ä Ö specifically is ≤ 2%, with no systematic substitution (e.g. å→a, ä→a, ö→o) in the error report.

**SV-04 Mixed Swedish/English text — MUST**
- [ ] With source "Auto (Swedish/English)", every block is language-identified. Blocks already in the target language are shown unchanged (TR-07).
- [ ] Blocks under 20 characters, where language identification is unreliable, are treated as the non-target language of the pair.
- [ ] On fixture `mixed-sv-en` (Swedish UI containing English product names and terms), ≥ 90% of blocks get the correct language decision.

**SV-05 Preserved tokens — MUST**
- [ ] Numbers, amounts, percentages, dates, times, URLs, e-mail addresses and ID numbers in the output contain the same digits in the same order as the source. Separators may be localized (e.g. `1 234,50 kr` → `SEK 1,234.50`) only if the value is unchanged.
- [ ] Tested on fixture `numbers-sv` and its English counterpart.

**SV-06 Compound words — SHOULD**
- [ ] Rendered Swedish text is tagged `lang="sv"` so the webview can hyphenate long compounds where it supports Swedish hyphenation; where it doesn't, words break at the block edge instead of overflowing it.
- [ ] On fixture `long-compounds-en` (English → Swedish), no translated block overflows its box by more than REN-02 allows.

**SV-07 Translation quality — SHOULD**
- [ ] The project maintains evaluation sets `tests/eval/sv-en` and `tests/eval/en-sv`: 200 segments each of UI, web and document text, written for the project under CC0, with reference translations.
- [ ] Each release reports a chrF score per backend on both sets in the release notes (tracked, not release-gating).
- [ ] Before 1.0, two native Swedish speakers rate 50 random on-device outputs per direction; mean adequacy is ≥ 4 out of 5.

**SV-08 Swedish user interface — MUST**
- [ ] The app's own UI (lens, settings, model manager, dialogs, tray menu, PRIVACY.md) ships in English and Swedish at 1.0.
- [ ] The UI follows the OS display language and can be overridden in Settings.

**SV-09 Swedish keyboard layout — MUST**
- [ ] With the Swedish layout active on Windows and Linux, all AltGr characters (@ £ $ € { [ ] } \ | ~ µ) still type correctly in other apps while Polyglass runs.
- [ ] Swedish letters typed in Polyglass's own text fields (e.g. LibreTranslate URL, search in model manager) work on all platforms.

**SV-10 Swedish on online backends — MUST**
- [ ] DeepL and Google backends pass both evaluation sets (SV-07) end-to-end without errors.
- [ ] Settings has an English-variant choice (UK/US), mapped to DeepL's separate British and American English targets. The default is US English (D-5).
- [ ] When a LibreTranslate server is added, the app reads its language list; if Swedish is not offered, it says so and doesn't enable the backend for Swedish pairs.

## 7. Non-functional requirements

### 7.1 Performance (PERF) — measured per §5

| ID | Requirement | Target REF-A | Target REF-B | Priority |
|---|---|---|---|---|
| PERF-01 | At rest → translation drawn, uncached, on-device (`ui-dialog-sv`, sv→en) | p50 ≤ 600 ms, p95 ≤ 1,200 ms | p50 ≤ 500 ms, p95 ≤ 1,000 ms | MUST |
| PERF-02 | At rest → translation drawn, cached | p95 ≤ 100 ms | p95 ≤ 100 ms | MUST |
| PERF-03 | OCR stage alone (600×300 px), Swedish source | p50 ≤ 200 ms | p50 ≤ 250 ms (Tesseract) | SHOULD |
| PERF-04 | Lens drag stays smooth | ≥ 55 fps, no dropped input | ≥ 55 fps | MUST |
| PERF-05 | Cold start to lens visible | ≤ 1 s | ≤ 1 s | MUST |
| PERF-06 | en→sv on `ui-dialog-en` meets the PERF-01 targets | same as PERF-01 | same as PERF-01 | MUST |

Online backend round trips are excluded from PERF-01 and PERF-06.

### 7.2 Resources (RES)

| ID | Requirement | Target | Priority |
|---|---|---|---|
| RES-01 | CPU, lens hidden | < 0.5% of one core (5-min average) | MUST |
| RES-02 | CPU, lens visible over static content | < 3% of one core (5-min average) | MUST |
| RES-03 | Memory, all app processes incl. webview, no model loaded | < 150 MB | MUST |
| RES-04 | Memory, sv→en and en→sv both loaded | < 400 MB | MUST |
| RES-05 | Unused models unloaded after 10 min idle | Memory returns within 10% of RES-03 | SHOULD |

### 7.3 Size (SIZE)

| ID | Requirement | Target | Priority |
|---|---|---|---|
| SIZE-01 | Installer, Windows (MSI/NSIS), no models | ≤ 15 MB | MUST |
| SIZE-02 | Installer, macOS (DMG), incl. bundled Tesseract + Swedish/English data | ≤ 20 MB | MUST |
| SIZE-03 | Linux .deb/.rpm (system WebKitGTK + Tesseract as dependencies) | ≤ 15 MB | MUST |
| SIZE-04 | Linux AppImage (self-contained) | ≤ 100 MB | SHOULD |
| SIZE-05 | sv→en plus en→sv on-device models | ≤ 100 MB combined | SHOULD |

### 7.4 Security (SEC)

- **SEC-01 — MUST:** Tauri capabilities are least-privilege; only commands the UI needs are exposed, and every IPC input is validated in Rust.
- **SEC-02 — MUST:** The webview loads only bundled assets with a strict Content-Security-Policy (no remote scripts, styles or frames).
- **SEC-03 — MUST:** All HTTP uses TLS (rustls); certificate validation can't be disabled from the UI.
- **SEC-04 — MUST:** Updates are opt-in and signed (Tauri updater signatures); unsigned updates are rejected.
- **SEC-05 — MUST:** CI runs `cargo deny` (advisories + licenses) and `npm audit`; release is blocked on unresolved high/critical advisories.

### 7.5 Accessibility and localization (A11Y, I18N)

- **A11Y-01 — MUST:** Settings and model manager meet WCAG 2.1 AA contrast and are fully keyboard-operable.
- **A11Y-02 — SHOULD:** Toolbar and settings work with NVDA (Windows), VoiceOver (macOS) and Orca (Linux), in both English and Swedish UI.
- **I18N-01 — MUST:** All UI strings are externalized (Fluent). English and Swedish ship at 1.0 (SV-08); other UI languages are accepted from the community via PR.

### 7.6 Open source and quality (OSS, QUAL)

- **OSS-01 — MUST:** Source under an OSI-approved license; `THIRD_PARTY_NOTICES` generated from dependencies on every release (including bundled Tesseract and its data on macOS).
- **OSS-02 — MUST:** Releases are built only in CI from signed tags, with `SHA256SUMS`, CycloneDX SBOM and build-provenance attestations attached.
- **OSS-03 — MUST:** README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, PRIVACY docs present; PRIVACY.md is linked from Settings and available in Swedish.
- **OSS-04 — SHOULD:** Release binaries for Windows and macOS are code-signed (and notarized on macOS) from the first public beta.
- **QUAL-01 — MUST:** CI passes `fmt`, `clippy -D warnings`, unit, fixture and pipeline tests on all three OSes, including the Swedish fixtures.
- **QUAL-02 — MUST:** CI fails on > 20% regression in `pg-bench` p50 latency versus `main`.
- **QUAL-03 — SHOULD:** Core crates (`pg-*`) have ≥ 70% line coverage.

## 8. Out of scope for v1

- Live translation of moving video (> 4 Hz content change).
- Handwriting and vertical CJK text recognition.
- Swedish-specific testing of pairs other than sv↔en.
- Text-to-speech, mobile apps, browser extension.
- Replacing text inside other applications.
- Required GPU acceleration.
- Mac App Store / Microsoft Store distribution.
- Live (non-Freeze) capture on Wayland.
- Apple's Translation framework or other OS-specific translation engines: every OS uses the same on-device backend (D-5).

## 9. Traceability to milestones

| Milestone | Requirements that must pass |
|---|---|
| M0 Spikes | ADRs for capture, click-through, on-device backend, macOS Swedish OCR (Tesseract vs Vision); Swedish fixtures created; `pg-bench` running; QUAL-01 skeleton |
| M1 MVP (Win + mac) | LENS-01–06, MODE-01, CAP-01–05, OCR-01–04, OCR-06, TR-01 (on-device), TR-02, TR-04, TR-06, TR-07, REN-01, REN-02, REN-04, REN-05, MDL-01–03, MDL-05, MDL-06, SET-01, PRIV-01, PRIV-02, PRIV-05–07, SV-01–05, SV-09, PERF-01, PERF-02, PERF-05, PERF-06 |
| M2 Linux + modes | MODE-02, MODE-03, CAP-02 (X11), CAP-06, HK-03, OCR-05, SV-02 (Linux), SIZE-03, SIZE-04 |
| M3 Online backends | TR-01 (all), TR-03, TR-05, SET-02, PRIV-03, PRIV-04, PRIV-08, SEC-03, SV-10 |
| M4 Polish | LENS-07, HK-01, HK-02, REN-06, SV-06, SV-08, A11Y-01, A11Y-02, I18N-01, MDL-04 |
| M5 1.0 | All MUSTs; SV-07; PERF-*, RES-*, SIZE-*, SEC-*, OSS-*, QUAL-* |

## 10. Open questions

All answered by the owner on 2026-11-01 (D-5):

1. ~~Which direction matters more?~~ **Swedish → English**, for now (SV-01). Pairs must stay configurable (MDL-07).
2. ~~Which languages next?~~ The main audience is English speakers, so the next pair is **German → English** (§3).
3. ~~UK or US English?~~ **US** by default (SV-10).
4. ~~Panel by default on Linux X11?~~ **Yes** (MODE-02).
5. ~~On-device fallback when online fails?~~ **Yes, with a notice in the lens** (TR-06). On-device stays the default at launch; online backends are opt-in (TR-02).
6. ~~Apple's Translation framework?~~ **No.** Same backend on every OS (§8).
