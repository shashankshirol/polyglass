# Polyglass

> Working name — see [CHECKLIST.md](./CHECKLIST.md) decision D-1.

Polyglass shows a movable lens over your desktop. Text under the lens is recognized and
translated in near real time, and the translation is drawn inside the lens. Everything can run
**on your device**; online translation backends are opt-in. Swedish ↔ English is the primary
language pair for v1.

**Status:** pre-alpha (milestone M0). The headless core (`crates/pg-*`) builds and is tested;
the desktop app, platform capture/OCR and the on-device translator are not implemented yet.
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

Requirements: Rust (stable, 1.85+). Node 20+ will be needed once the desktop app lands.

```
cargo test --workspace
cargo run --release -p pg-bench -- --fixture tests/fixtures/ui-dialog-sv
```

`pg-bench` runs the pipeline on a fixture and prints per-stage latency percentiles. Until real
OCR/translation engines land it uses mock engines, which measures pipeline overhead only.

## Repository layout

```
crates/pg-core       shared types (geometry, frames, lines/blocks, languages, errors)
crates/pg-capture    ScreenCapturer trait (+ fixture capturer; per-OS backends to come)
crates/pg-ocr        OcrEngine trait, default-engine table, language resolution
crates/pg-translate  Translator trait, sv/en language ID, preserved-token checks, pivoting
crates/pg-pipeline   scheduler, change detection, block grouping, cache, live driver
crates/pg-models     model manifest, SHA-256 verification, store, resumable downloads
crates/pg-bench      benchmark CLI
models/manifest.toml pinned model URLs + hashes + licenses
tests/fixtures/      ground-truth fixtures (CC0)
docs/                architecture notes and ADRs
```

## License

Intended: MIT OR Apache-2.0 (pending decision D-2). Fixtures under `tests/fixtures` are CC0-1.0.
