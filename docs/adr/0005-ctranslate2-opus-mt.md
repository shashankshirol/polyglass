# ADR-0005: Move on-device translation to CTranslate2 + OPUS-MT

- **Status:** accepted (owner decision D-8, 2026-11-01); implementation tracked as M1-TR-6
- **Date:** 2026-11-01
- **Supersedes:** [ADR-0004](./0004-on-device-translation.md) (its option C, `fxtranslate` +
  Mozilla models)
- **Requirements:** TR-01, TR-02, TR-03, SV-01, SV-05, MDL-01, MDL-02, MDL-06, MDL-07, SIZE-05,
  PERF-01, PERF-06, RES-04

## Context

ADR-0004 chose `fxtranslate` running Mozilla's Firefox Translations models. It was the fastest
and smallest option measured. But the owner asked for a runtime that is widely used and
proven:
- `fxtranslate` is young: first released 2026-07, version 0.5, one maintainer, and published
  from a personal fork rather than by Mozilla.
- The alternative that runs the same model files is the Bergamot C++ engine. Its old repository,
  `browsermt/bergamot-translator`, looks stale. Its current home, `mozilla/translations/inference`,
  has commits up to 2026-01, but they are mostly dependency bumps. Firefox ships its WASM build,
  not the native one we would use.

SPEC v0.3 also asks for configurable language pairs (MDL-07), with German → English as the
next pair after sv↔en. Whatever we choose must have models for many pairs, not only Swedish.

## Options

The measurements are from ADR-0004 and were taken on this machine (Ryzen 7 7800X3D, Windows 11).

| Option | Pros | Cons | Measurements |
|---|---|---|---|
| A. Keep `fxtranslate` + Mozilla models | Fastest and smallest; plain Cargo build | Young runtime with one maintainer (the owner's concern) | sv→en 7.4 ms short, 95 ms paragraph; 68 MB RSS; 30.5 MB of models |
| B. Bergamot C++ + Mozilla models | Same models as A; engine behind Firefox Translations | Native build lightly maintained; heavy CMake build with submodules; MKL/OpenBLAS issue on x86; needs our own `cxx` bridge and an `unsafe` exception | Not built (ADR-0004) |
| **C. CTranslate2 (`ct2rs`) + OPUS-MT** | CTranslate2 (OpenNMT) is actively released (v4.8.4, 2026-10) and widely deployed (faster-whisper, Argos Translate). OPUS-MT (Helsinki-NLP) has models for well over a thousand pairs, including de→en. The `cxx` bridge stays inside `ct2rs`, so no `unsafe` in our code | Models about 5× larger; 2–4× slower than A; CMake + MSVC developer environment; we convert and host the CTranslate2 files ourselves; CC-BY-4.0 needs attribution; `ct2rs` has one maintainer (thin binding, can be vendored or replaced) | sv→en 26.5 ms short, 199 ms paragraph (4 threads); 200 MB RSS with both loaded; ≈ 162 MB of models (int8, both directions); clean build 60 s |
| D. CTranslate2 + `facebook/m2m100_418M` | One MIT model for 100 languages | 1.94 GB (fp32); estimated 0.4–0.5 GB as int8; expected to be much slower and weaker per pair | Not measured; the owner dropped this option |
| E. `candle` (Hugging Face) + OPUS-MT, or ONNX Runtime (`ort`) | Pure Rust (candle) / Microsoft runtime (ORT) | No measurements; candle has no M2M-100; `ort` downloads prebuilt native libraries | Not measured |

## Decision

**Option C: CTranslate2 through `ct2rs`, with OPUS-MT models converted to CTranslate2 int8.**
This is the most proven runtime that is actively maintained, it already built and ran here in
the ADR-0004 spike, and OPUS-MT covers the configurable-pairs goal through per-pair models. Its
measured latency is still well inside PERF-01 (p50 ≤ 600 ms).

The rest of the design does not change:
- the `Translator` trait and `TokenGuard` (SV-05);
- `pg-models` (manifest, SHA-256, resumable download, store);
- the lens download flow.

`fxtranslate`, the Mozilla model entries and their code are removed once the new backend is
verified, rather than kept as a second engine.

## Consequences

- **Size:** SIZE-05 is raised from ≤ 100 MB to ≤ 200 MB for both sv↔en directions together;
  ≈ 162 MB is expected.
- **Memory:** RES-04 (< 400 MB) still holds at ≈ 200–260 MB.
- **Build:** every OS needs CMake and a C++17 compiler, and Windows needs the MSVC developer
  environment. CI and the README must cover this. ADR-0004 found two Windows problems: a long
  target path breaks sentencepiece's try-compile, and only the `ruy` backend links cleanly. The
  build must work with a plain `npm run tauri dev` / `cargo build` from a normal terminal, or
  the README must explain the one-time setup.
- **Models:** OPUS-MT ships Marian `.npz` files, which CTranslate2's Python converter turns into
  int8. The repo gets a pinned, reproducible conversion script. The files it produces need an
  https host so the manifest can point at them (MDL-02); where to host them is owner decision
  D-9. Until then, models can be installed through local import (MDL-04).
- **License:** OPUS-MT models are CC-BY-4.0 (the LICENSE file in the release zip; the Hugging
  Face cards say Apache-2.0, so we use the stricter one). Attribution is shown in the model
  manager and recorded in the manifest.
- **Must be re-verified on the new engine:**
  - the SV-05 guard: ADR-0004 saw CTranslate2 change amounts and translate domains, and
    `TokenGuard`'s placeholders must survive OPUS-MT;
  - M1-TR-1..4;
  - PERF-01/06.
- **Threads:** CTranslate2 needs several threads to reach these numbers (4 here). The thread
  count must be capped so the lens stays light on smaller machines.
- **Follow-ups:** macOS/Linux/arm64 builds remain untested (M0-SPK-3 stays `[~]`); German → English
  (M3-9) uses the same backend.
