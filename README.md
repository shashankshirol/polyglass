# Polyglass

> Working name — see [CHECKLIST.md](./CHECKLIST.md) decision D-1.

Polyglass shows a movable lens over your desktop. Text under the lens is recognized and
translated in near real time, and the translation is drawn inside the lens. Everything can run
**on your device**; online translation backends are opt-in. Swedish ↔ English is the primary
language pair for v1.

**Status:** pre-alpha (milestone M0/M1). The headless core (`crates/pg-*`) builds and is tested.
The desktop lens window (`apps/desktop`) runs on Windows with real screen capture (Windows
Graphics Capture), Windows OCR and on-device Swedish ↔ English translation (Mozilla's Firefox
Translations models, [ADR-0004](./docs/adr/0004-on-device-translation.md)). macOS and Linux
capture/OCR are not implemented yet.
See [CHECKLIST.md](./CHECKLIST.md) for progress.

## Documents

| Document | What it is |
|---|---|
| [SPEC.md](./SPEC.md) | Product requirements with IDs and acceptance criteria |
| [PLAN.md](./PLAN.md) | Architecture, decisions, roadmap |
| [CHECKLIST.md](./CHECKLIST.md) | Development tracker and session log |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | How the code is organized |
| [PRIVACY.md](./PRIVACY.md) | What data goes where |

## Build and run locally

Only Windows runs the full app today (live capture, OCR and translation). On macOS and Linux the
core crates build and test, and the app runs in demo mode only.

### 1. Prerequisites

- **Rust** stable, 1.90 or newer, via [rustup](https://rustup.rs). On Windows use the MSVC
  toolchain (`x86_64-pc-windows-msvc`).
- **Node.js** 20 or newer (npm comes with it).
- **The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS:**
  - Windows: the Microsoft C++ Build Tools ("Desktop development with C++") and the WebView2
    runtime, which Windows 11 already has.
  - Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.
  - macOS: the Xcode command-line tools.
- **Swedish text recognition (Windows, optional):** Settings → Time & language → Language &
  region → add Swedish → Language options → install its optical character recognition feature.
  Without it the lens reads English only and shows ⚠ SV in the toolbar.

### 2. Run the app in development mode

From the repository root:

```
cd apps/desktop
npm install
npm run tauri dev
```

The first run compiles the whole workspace, which takes a few minutes. After that the UI reloads
when you save a change, and Rust changes trigger a rebuild. The lens opens centred on the
monitor your cursor is on, and a tray icon appears.

To run demo mode, which uses mock capture, OCR and translation and never reads the screen:

```
# PowerShell
$env:POLYGLASS_DEMO = "1"; npm run tauri dev
# bash/zsh
POLYGLASS_DEMO=1 npm run tauri dev
```

In PowerShell, run `Remove-Item Env:POLYGLASS_DEMO` to go back to live mode in the same terminal.

### 3. First run: download the translation models

The translation models aren't bundled with the app. On first launch the lens says they're
missing, and the toolbar shows ⬇. Click it to download both directions:
- about 30 MB in total, from Mozilla's model storage;
- each file is checked against its SHA-256;
- they're saved to `%LOCALAPPDATA%\org.polyglass.desktop\models`;
- translation starts as soon as the download finishes.

Later launches load the models from disk in under a second, and nothing else goes online.

### 4. Use the lens

- Drag the toolbar to move the lens, or drag any edge or corner to resize it. With the lens
  focused, the arrow keys move it by 1 px, and Shift+arrow moves it by 10 px.
- Put the lens over Swedish text. The translation is drawn in place once the lens stops
  moving.
- The toolbar buttons:
  - ❚❚ pauses translation;
  - ⇲ turns click-through on or off, so clicks reach the window underneath;
  - ✕ hides the lens.
- The tray icon menu has **Show/hide lens** and **Quit Polyglass**. While the lens is hidden,
  nothing is captured.

### 5. Release build

```
cd apps/desktop
npm run tauri build -- --no-bundle
```

This builds the UI and an optimized app binary, `target\release\polyglass.exe` at the
repository root, which runs on its own: double-click it, or run it from a terminal. Add `--demo`
for demo mode. Running `npm run tauri build` without `--no-bundle` also creates the MSI and NSIS
installers under `target\release\bundle\`. The Tauri CLI downloads WiX and NSIS for that step,
which hasn't been tested yet.

### 6. Tests and benchmarks

From the repository root:

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p pg-bench -- --fixture tests/fixtures/ui-dialog-sv
```

`pg-bench` runs the pipeline on a fixture and prints latency percentiles for each stage:
- the default, `--ocr mock`, measures only the pipeline's own overhead;
- `--ocr windows` runs Windows OCR;
- translation in `pg-bench` is still a mock;
- until the fixture images exist, it renders the fixture text with a system font.

Tests against the real translation models download those models first, so they're ignored by
default:

```
cargo test -p pg-bergamot --release --test real_models -- --ignored --test-threads 1
```

### Troubleshooting

- **`cargo metadata ... program not found` from the Tauri CLI (Windows):** your `cargo.exe` is
  probably a rustup symlink that child processes can't start. Put the toolchain's own `bin` first
  on `PATH`, then retry:

  ```
  $env:PATH = "$(rustc --print sysroot)\bin;$env:PATH"
  ```

- **"The lens can't hide itself from screen capture … live capture is off":** live mode starts
  only when Windows can exclude the lens window from its own capture, which needs Windows 10
  version 2004 or newer. Use demo mode on older systems.
- **⚠ SV in the toolbar:** the Swedish OCR feature isn't installed (see step 1). Click ⚠ SV to
  open the Windows language settings.

## Repository layout

```
apps/desktop         Tauri 2 shell (src-tauri/) + lens UI (ui/, TypeScript + Vite, no framework)
crates/pg-core       shared types (geometry, frames, lines/blocks, languages, errors)
crates/pg-capture    ScreenCapturer trait, Windows Graphics Capture, fixture capturer
crates/pg-ocr        OcrEngine trait, default-engine table, language resolution, Windows OCR
crates/pg-translate  Translator trait, sv/en language ID, preserved-token checks + guard, pivoting
crates/pg-bergamot   on-device sv↔en translation (Mozilla models, fxtranslate engine)
crates/pg-pipeline   scheduler, change detection, block grouping, cache, live driver
crates/pg-models     model manifest, SHA-256 verification, store, resumable downloads
crates/pg-bench      benchmark CLI
crates/pg-testkit    test helpers (renders text into frames with a system font)
models/manifest.toml pinned model URLs + hashes + licenses
tests/fixtures/      ground-truth fixtures (CC0)
docs/                architecture notes and ADRs
```

## License

Intended: MIT OR Apache-2.0 (pending decision D-2). Fixtures under `tests/fixtures` are CC0-1.0.
