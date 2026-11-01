# ADR-0005: Move on-device translation to CTranslate2 + OPUS-MT

- **Status:** accepted (owner decision D-8, 2026-11-01); implemented and verified on Windows in
  M1-TR-6 (session 8, 2026-11-01)
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
verified, rather than kept as a second engine (done in M1-TR-6).

## Consequences

Measured results from the implementation (M1-TR-6) are in "Verified on this machine" below.

- **Size:** SIZE-05 is raised from ≤ 100 MB to ≤ 200 MB for both sv↔en directions together.
  Measured: 84.8 MB per direction, **169.6 MB** together (the newer `opus+bt` releases have a
  slightly larger vocabulary than the 2020 ones ADR-0004 measured).
- **Memory:** RES-04 (< 400 MB) still holds: **211 MB** RSS with both directions loaded,
  275 MB after the latency runs (test process); the release app's working set was 294 MB with
  both loaded (126 MB with the ADR-0004 models; WebView2 processes not counted).
- **Build:** every OS needs CMake and a C++17 compiler. CI and the README cover this. On Windows
  a normal terminal works without the MSVC developer environment once CMake and Ninja are on
  `PATH` (one-time `winget install Kitware.CMake Ninja-build.Ninja`); `.cargo/config.toml`
  handles the rest:
  - `+crt-static` for MSVC targets: ct2rs builds CTranslate2 with `/MT`, and the default `/MD`
    Rust build fails to link (LNK2038).
  - `CMAKE_GENERATOR_<msvc target> = Ninja`: without a generator, cmake-rs uses the Visual
    Studio generator and overwrites `CMAKE_CXX_FLAGS_RELEASE` without `/O2`, which made
    translation ≈ 13× slower (341 ms instead of 28.6 ms for a short string). cmake-rs passes the
    MSVC environment it finds to CMake, so Ninja needs no `vcvars64`. (`_CL_=/O2` instead breaks
    CMake's Debug compiler probe, `/RTC1`.)
  - The long-path try-compile failure didn't occur with the repository at `E:\Dev\Polyglass`; the
    README advises a short checkout path. Clean debug build of `pg-ct2` with Ninja ≈ 1 min.
- **Models:** OPUS-MT ships Marian `.npz` files, which CTranslate2's Python converter turns into
  int8. `scripts/models/opus_mt.py` does this with a hash-locked converter (ctranslate2 4.8.4,
  the version ct2rs 0.10.3 bundles) and pins the SHA-256 of its inputs (the official release
  zips: `swe-eng/opus+bt-2021-04-30`, `eng-swe/opus+bt-2021-04-14`, the newest pair-specific
  releases) and its outputs; two runs in fresh venvs gave identical files. The files need an
  https host so the manifest can point at them (MDL-02); where to host them is owner decision
  D-9, decided 2026-11-01: assets of a GitHub pre-release of this repository (M1-TR-7,
  `models-opus-mt-2026-11-01`, built on a clean runner by `.github/workflows/models-release.yml`
  with the same pinned script). Entries without `url` stay possible (import-only, a small
  `pg-models` change); the same script installs the models offline through `import-models`
  (MDL-04).
- **License:** OPUS-MT models are CC-BY-4.0 (the LICENSE file in the release zip; the Hugging
  Face cards say Apache-2.0, so we use the stricter one). Attribution is shown in the model
  manager and recorded in the manifest.
- **Re-verified on the new engine:**
  - the SV-05 guard: `{n}` placeholders survive OPUS-MT in all 20 masked blocks of
    `numbers-sv`/`numbers-en`, so the masking is unchanged; all 20 blocks pass
    `check_preserved` through `TokenGuard` (5 masked retries, 0 fallbacks);
  - M1-TR-1..4 (tests below);
  - PERF-01/06: translation of the whole `ui-dialog-sv` / `ui-dialog-en` takes p50 147–198 /
    138–185 ms (two runs), inside the 600 ms cycle budget with OCR at ≈ 19 ms. The live app's
    uncached cycle over a Swedish dialog line was 117 ms (OCR 8.6 + translation 107.6 ms).
- **Threads:** CTranslate2 needs several threads to reach these numbers. Each model uses
  `Options::default_threads()`: half the logical CPUs, at least 1 and at most 4 (4 here).
## Verified on this machine (M1-TR-6, session 8)

Windows 11, Ryzen 7 7800X3D, Rust 1.99.1, MSVC 14.51 (VS 18) with CMake 4.3 + Ninja, release
builds, greedy decoding, 4 threads per model. Latency = 3 warm-up runs, then 30 timed runs,
nearest-rank p50 / p95, through `TokenGuard`; two runs of the same test:

| | Run 1 | Run 2 |
|---|---|---|
| sv→en short (`Spara ändringar?`) | 28.6 / 33.6 ms | 27.6 / 29.6 ms |
| sv→en paragraph (`paragraphs-sv` block 2) | 212 / 226 ms | 156 / 166 ms |
| sv→en `ui-dialog-sv` (8 blocks) | 198 / 209 ms | 147 / 155 ms |
| en→sv short (`Save changes?`) | 37.5 / 42.5 ms | 27.9 / 32.4 ms |
| en→sv paragraph | 209 / 220 ms | 154 / 161 ms |
| en→sv `ui-dialog-en` (8 blocks) | 185 / 198 ms | 138 / 147 ms |
| Load, both directions | 0.27 s | 0.27 s |
| RSS: both loaded / after runs | 211 / 275 MB | 211 / 275 MB |

Tests (`crates/pg-ct2/tests/real_models.rs`, `#[ignore]`d, all pass):
`mdl04_tr2_converted_models_import_and_verify`, `tr1_tr3_both_directions_loaded_and_translate`
(sv→en / en→sv swap without reloading: 58–86 ms per pair of short strings),
`sv05_placeholders_survive_opus_mt` (20/20), `tr4_sv05_numbers_fixtures_keep_preserved_tokens`
(20/20; e.g. `Totalt att betala: 1 234,50 kr` → `Total to pay: 1,234.50`),
`perf_latency_and_memory_both_directions`.

Live app (release, `lens-read.ps1`): "Vill du spara ändringarna innan du stänger?" → "Do you
want to save the changes before closing?", English passed through; models loaded in 0.25 s at
startup; uncached cycle 117 ms (capture 0.8, OCR 8.6, translation 107.6 ms), cached 10 ms.
Without the models the lens showed the install command (no ⬇, since the entries have no URLs).

Not measured: macOS, Linux, arm64; `accelerate` on macOS; translation quality beyond spot checks
(SV-07). Spot checks show the usual small-model slips (en→sv `save them` → `rädda dem`).

- **Follow-ups:** macOS/Linux/arm64 builds remain untested (M0-SPK-3 stays `[~]`); German → English
  (M3-9) uses the same backend.
