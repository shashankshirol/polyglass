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

## Building (developers)

Requirements: Rust (stable, 1.85+), Node 20+, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS (WebView2 on
Windows; webkit2gtk 4.1 on Linux).

```
cargo test --workspace
cargo run --release -p pg-bench -- --fixture tests/fixtures/ui-dialog-sv
```

Desktop app (lens window). On Windows it captures and recognizes the text under the lens by
default. Demo mode (`POLYGLASS_DEMO=1`, or the `--demo` argument on the built binary) swaps in
mock capture/OCR/translation and never reads the screen:

```
cd apps/desktop
npm install
npm run tauri dev                      # live capture + Windows OCR
POLYGLASS_DEMO=1 npm run tauri dev     # mocks only
```

Translation models aren't bundled (MDL-06). On first use the lens says they're missing and the
toolbar shows ⬇; it downloads both directions (about 30 MB, from Mozilla's model storage, SHA-256
checked) into `%LOCALAPPDATA%\org.polyglass.desktop\models` and starts translating.

Swedish text recognition on Windows needs the optional Swedish OCR feature (Settings → Time &
language → Language & region → Swedish → Language options); without it the lens reads English
only and says so.

On Windows, if the Tauri CLI reports
`cargo metadata ... program not found`, your `cargo.exe` is probably a rustup symlink that child
processes can't start; put `$(rustc --print sysroot)\bin` first on `PATH`.

`pg-bench` runs the pipeline on a fixture and prints per-stage latency percentiles. Until the
fixture images exist it renders the fixture text with a system font. `--ocr mock` (default)
measures pipeline overhead only; `--ocr windows` runs Windows OCR. Translation in `pg-bench` is
still a mock.

Tests against the real translation models download them first, so they are ignored by default:

```
cargo test -p pg-bergamot --release --test real_models -- --ignored --test-threads 1
```

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
