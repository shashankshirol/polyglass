# Polyglass

Polyglass shows a movable lens over your desktop. Text under the lens is recognized and
translated in near real time, and the translation is drawn inside the lens. Everything can run
**on your device**; online translation backends are opt-in. v1 is built and tested for
Swedish ↔ English. German → English is also in the catalog, and you can
[set it up yourself](#optional-german--english). Language pairs come from the model catalog
(`models/manifest.toml`), so a new pair is a catalog entry, not a code change.

**Status:** pre-alpha (milestone M1). **v1 is Windows only.** The desktop lens window
(`apps/desktop`) runs with real screen capture (Windows Graphics Capture), Windows OCR and
on-device Swedish ↔ English translation with CTranslate2 and Helsinki-NLP's OPUS-MT models
([ADR-0005](./docs/adr/0005-ctranslate2-opus-mt.md)). The models aren't hosted for download yet,
so you convert and install them once with a script (step 3). macOS and Linux may come after v1;
capture and OCR sit behind traits so they can be added
([docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md#adding-a-platform)). See
[CHECKLIST.md](./CHECKLIST.md) for progress.

## Documents

| Document | What it is |
|---|---|
| [SPEC.md](./SPEC.md) | Product requirements with IDs and acceptance criteria |
| [PLAN.md](./PLAN.md) | Architecture, decisions, roadmap |
| [CHECKLIST.md](./CHECKLIST.md) | Development tracker and session log |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | How the code is organized |
| [PRIVACY.md](./PRIVACY.md) | What data goes where |

## Build and run locally

Polyglass builds and runs on Windows 10 2004 or newer (x64).

### 1. Prerequisites

- **Rust** stable, 1.90 or newer, via [rustup](https://rustup.rs), with the MSVC toolchain
  (`x86_64-pc-windows-msvc`).
- **Node.js** 20 or newer (npm comes with it).
- **The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/):** the Microsoft C++
  Build Tools or Visual Studio 2022 or newer with "Desktop development with C++", and the
  WebView2 runtime, which Windows 11 already has.
- **CMake 3.x or newer and Ninja**, on `PATH`. The translation engine (CTranslate2, through the
  `ct2rs` crate) is C++ and is compiled during the first build. One-time setup:
  `winget install Kitware.CMake Ninja-build.Ninja`, then open a new terminal. The C++ compiler
  is the MSVC one from the Build Tools above; the build finds it by itself, so a normal
  PowerShell terminal works (no Developer Prompt needed). Visual Studio also ships CMake and
  Ninja, but they aren't on `PATH` outside the Developer Prompt.
- **Python 3.12 or newer**, only to convert the translation models (step 3).
- **Swedish text recognition (optional):** Settings → Time & language → Language &
  region → add Swedish → Language options → install its optical character recognition feature.
  Without it the lens reads English only and shows ⚠ SV in the toolbar.

### 2. Run the app in development mode

From the repository root:

```
cd apps/desktop
npm install
npm run tauri dev
```

The first run compiles the whole workspace, including CTranslate2 and SentencePiece through
CMake, which takes a few minutes. After that the UI reloads when you save a change, and Rust
changes trigger a rebuild. The lens opens centred on the monitor your cursor is on, and a tray
icon appears.

`.cargo/config.toml` does two things for CTranslate2, so nothing else needs setting
up: it links the static C runtime (`+crt-static`), which CTranslate2 requires, and it makes
CMake use Ninja. With the Visual Studio generator, the build tooling (cmake-rs) drops the
optimization flags and translation is about 13× slower. If you set a `RUSTFLAGS` environment
variable, it replaces the first setting, so add `-C target-feature=+crt-static` to it.

To run demo mode, which uses mock capture, OCR and translation and never reads the screen:

```
# PowerShell
$env:POLYGLASS_DEMO = "1"; npm run tauri dev
# bash/zsh
POLYGLASS_DEMO=1 npm run tauri dev
```

In PowerShell, run `Remove-Item Env:POLYGLASS_DEMO` to go back to live mode in the same terminal.

### 3. First run: get the translation models

The translation models aren't bundled with the app (MDL-06). The converted OPUS-MT models are
published as assets of a **GitHub pre-release of this repository**,
[`models-opus-mt-2026-11-01`](https://github.com/shashankshirol/polyglass/releases/tag/models-opus-mt-2026-11-01)
(decision D-9), and `models/manifest.toml` lists each file's URL and SHA-256.

On first launch a welcome window offers both directions (about 170 MB together) with
**Download** and **Not now**; nothing is downloaded until you choose Download. Each file is
checked against its SHA-256 before it's saved to the model store,
`%LOCALAPPDATA%\org.polyglass.desktop\models` on Windows. Translation starts once the download
finishes, without a restart. Later, ⚙ in the lens toolbar (or ⬇ while the models are missing,
or "Translation models…" in the tray menu) opens the model manager: download with progress,
cancel and resume, delete, licenses and disk usage.

The repository is private until the first app release, so the models release can't be
downloaded without a GitHub login: after a download attempt the window and the lens say the
release isn't reachable and show the install command instead. Until then, install the models
with the script below.

To build the same files yourself (offline install, or a check of the release), run this from
the repository root:

```
python scripts/models/opus_mt.py
```

This one command:
- creates a Python venv under `target/opus-mt` with the pinned, hash-checked converter
  (`scripts/models/requirements.txt`);
- downloads Helsinki-NLP's official OPUS-MT sv→en and en→sv releases (2 × 286 MB) and checks
  their SHA-256;
- converts them to CTranslate2 int8 (about 20 s) and checks every output file against the
  SHA-256 in the script, so the result is identical on every machine and to the release assets;
- installs both directions (170 MB) into the app's model store through
  `cargo run -p pg-models --bin import-models`, which verifies them against
  `models/manifest.toml` again.

Model ids pick what to convert (see [German → English](#optional-german--english)); `--all`
converts every model the script knows (the models release workflow uses it).

Then restart Polyglass. Later launches load the models from disk, and nothing goes online. Use
`--clean` to delete the downloads and the venv afterwards, `--store <dir>` for another model
store, and `--no-install` to only convert. The OPUS-MT models are CC-BY-4.0: the attribution is
in `models/manifest.toml` and the release notes, and the license text is installed next to each
model.

**Publishing a new models release (maintainers):** point the manifest URLs at a new tag
`models-opus-mt-YYYY-MM-DD` first (`releases/download/<tag>/<model id>.<file name>`), check
locally with `python scripts/models/opus_mt.py --no-install` and
`python scripts/models/release_assets.py --tag <tag>`, commit, then push an annotated tag of that
name. `.github/workflows/models-release.yml` converts the models on a clean runner, checks every
file against the manifest and creates the pre-release with the license, attribution and
`SHA256SUMS`. It never replaces an existing release.

### Optional: German → English

German → English isn't part of v1's tested scope and isn't in a models release, so you set it
up yourself. It uses the same pinned, hash-checked steps as the Swedish models.

1. **Install the model** (Python 3.12 or newer; one 289 MB download, 86 MB installed). From the
   repository root:

   ```
   python scripts/models/opus_mt.py opus-mt-de-en
   ```

   Add `--clean` to delete the download and the converter venv afterwards. Restart Polyglass
   when it finishes.
2. **Add German text recognition.** Either of these works:
   - Settings → Time & language → Language & region → Add a language → German (Germany). In
     the next dialog, untick "Set as my Windows display language" and every optional feature
     except "Optical character recognition". Windows also adds a German keyboard layout; remove
     it under German → Language options → Keyboards if you don't want it.
   - Or, in a PowerShell terminal run as administrator, install only the OCR feature:

     ```powershell
     Add-WindowsCapability -Online -Name "Language.OCR~~~de-DE~0.0.1.0"
     ```

   Without it, the lens shows ⚠ DE (click it to open the language settings) and reads German
   text with the English recognizer, which reads most words but may get ä, ö, ü and ß wrong.
3. **Pick the pair:** in the lens toolbar, set the source to "German/English" (each block is
   detected, and English text is shown as it is) or "German only", and the target to English.

### 4. Use the lens

- Drag the toolbar to move the lens, or drag any edge or corner to resize it. With the lens
  focused, the arrow keys move it by 1 px, and Shift+arrow moves it by 10 px.
- Put the lens over Swedish text. The translation is drawn in place once the lens stops
  moving.
- The two pickers at the left of the toolbar choose the language pair: every pair in the model
  catalog, installed or not (⬇ marks a missing model; ⬇ or ⚙ in the toolbar downloads it). The
  source picker offers each language as "Swedish/English" (each block is detected; text already
  in the target language is shown as it is) or "Swedish only". The choice is saved in
  `settings.toml` (`[translation] source`, `target`, `auto`). OCR uses the source language: for
  German, see [German → English](#optional-german--english).
- The toolbar buttons:
  - ⇄ (Ctrl+Alt+S) swaps the direction when the catalog has the reverse pair;
  - ❚❚ pauses translation;
  - ⇲ turns click-through on or off, so clicks reach the window underneath;
  - ✕ hides the lens.
- The tray icon menu has **Show/hide lens**, **Translation models…** and **Quit Polyglass**;
  a left click on the icon shows or hides the lens. While the lens is hidden, nothing is
  captured.

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
- `--translator ct2` translates with the on-device models from the app's model store (install
  them with `python scripts/models/opus_mt.py`), so `--ocr windows --translator ct2` measures a
  whole uncached cycle as the lens runs it (PERF-01; add `--fixture tests/fixtures/ui-dialog-en
  --target sv` for PERF-06); the default `--translator mock` answers instantly;
- `--eval tests/eval/sv-en --translator ct2` translates an evaluation set instead and prints its
  chrF score (SV-07, `tests/eval/README.md`);
- it renders the fixture image from `truth.json` at `--scale` and `--theme`, and with
  `--ocr windows` it also prints OCR accuracy (CER, WER, å/ä/ö).

Tests against the real translation models need the converted models, so they're ignored by
default. Convert them first (they're imported into a test store, not the app's):

```
python scripts/models/opus_mt.py --no-install
cargo test -p pg-ct2 --release --test real_models -- --ignored --test-threads 1 --nocapture
```

`cargo test -p pg-models --test github_release -- --ignored --nocapture` checks the models release
on GitHub itself (the redirect to the asset host and a resumed download; ≈ 2 MB).

The MSRV is Rust 1.90: `cargo +1.90 check --workspace --all-targets`. `cargo deny check` checks
licenses and advisories.

### 7. Developer flags and crash reports

Pass these after `polyglass.exe`, or after `npm run tauri dev -- --`:

- `--trace-timings` logs each cycle's stage timings and, once the lens has painted a result,
  `at_rest_ms`: from the lens coming to rest to the result on screen (PERF-01/02/06), with a
  summary when the app quits. `--trace-timings=uncached` also clears the translation cache each
  time the lens moves, so every result is translated afresh. The toolbar shows TRACE. Only
  numbers are logged, never screen text. `apps/desktop/scripts/lens-trace.ps1` moves the lens
  over a fixture's text to collect the numbers; release builds have no console, so redirect
  the log, which goes to standard output (`Start-Process target\release\polyglass.exe -ArgumentList '--trace-timings=uncached'
  -RedirectStandardOutput trace.log`).
- `--debug-dump-frames[=DIR]` saves every captured frame as a PNG in a new folder under DIR
  (default: `debug-frames` in the app's local data folder, `%LOCALAPPDATA%\org.polyglass.desktop`
  on Windows), keeping the newest 200. This is the only way screen images reach the disk
  (PRIV-02), and the lens shows a red banner with the folder for as long as it runs.

If Polyglass panics it saves a crash report (version, OS, where and why it stopped, a
backtrace) in `crash-reports` in the same local data folder and keeps the newest 10. Nothing is
uploaded. When reports exist, the tray menu shows **Crash reports…**, which opens the folder, so
you can attach one to an issue. Crashes inside the C++ translation engine end the app without a
report.

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
- **"is `cmake` not installed?", or CMake can't find `ninja`, while building `ct2rs` or
  `sentencepiece-sys`:** CMake or Ninja isn't on `PATH` (see step 1). Open a new terminal after
  installing them, then run `cargo clean -p ct2rs -p sentencepiece-sys` so CMake starts over.
- **`LNK2038: mismatch detected for 'RuntimeLibrary'` (Windows):** a `RUSTFLAGS` environment
  variable replaced `.cargo/config.toml`'s `+crt-static`; see step 2.
- **Translation takes hundreds of milliseconds per short string (Windows):** CTranslate2 was
  built unoptimized with the Visual Studio generator, for example because a `CMAKE_GENERATOR`
  environment variable overrode `.cargo/config.toml`. Remove it, run
  `cargo clean -p ct2rs -p sentencepiece-sys` and build again.
- **CMake errors in a try-compile, or paths that are too long (Windows):** the sentencepiece
  build fails when the target directory path is very long. Keep the checkout (or
  `CARGO_TARGET_DIR`) at a short path such as `C:\src\polyglass`.
- **The lens says the models aren't installed after running the script:** restart Polyglass;
  models are loaded at startup.

## Repository layout

```
apps/desktop         Tauri 2 shell (src-tauri/) + lens UI (ui/, TypeScript + Vite, no framework)
crates/pg-core       shared types (geometry, frames, lines/blocks, languages, errors)
crates/pg-capture    ScreenCapturer trait, Windows Graphics Capture, fixture capturer
crates/pg-ocr        OcrEngine trait, language resolution, Windows OCR
crates/pg-translate  Translator trait, sv/en language ID, preserved-token checks + guard, pivoting
crates/pg-ct2        on-device translation: CTranslate2 (ct2rs) running OPUS-MT models
crates/pg-pipeline   scheduler, change detection, block grouping, cache, live driver
crates/pg-models     model manifest, SHA-256 verification, store, resumable downloads
crates/pg-bench      benchmark CLI
crates/pg-testkit    test helpers (fixture images from truth.json, CER/WER scoring)
models/manifest.toml pinned model files + hashes + licenses + GitHub Release URLs (D-9)
scripts/models/      pinned OPUS-MT → CTranslate2 int8 conversion + install script
tests/fixtures/      ground-truth fixtures (CC0)
docs/                architecture notes and ADRs
```

## License

[Apache-2.0](./LICENSE). Fixtures under `tests/fixtures` are CC0-1.0. Translation models are
installed separately, not bundled, and carry their own licenses (listed in
`models/manifest.toml`; the OPUS-MT sv↔en models by Helsinki-NLP are CC-BY-4.0).
