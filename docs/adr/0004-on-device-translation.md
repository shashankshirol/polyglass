# ADR-0004: On-device translation engine and models

- **Status:** accepted for Windows (macOS/Linux/arm64 builds and timings not run; see "Not
  measured")
- **Date:** 2026-10-31
- **Requirements:** TR-01, TR-02, SV-01, SV-05, MDL-01, MDL-02, MDL-06, SIZE-01, SIZE-05,
  PERF-01, PERF-06, RES-04

## Context

The lens needs an on-device sv→en and en→sv translator behind the `Translator` trait (TR-01),
fast enough that "at rest → translation drawn" stays within PERF-01 (p50 ≤ 600 ms with OCR at
≈ 19 ms), with both directions loaded at once (SV-01, RES-04 < 400 MB), models ≤ 100 MB for
the pair (SIZE-05), downloaded from manifest URLs with SHA-256 (MDL-02), and licenses on the
`deny.toml` allowlist. The workspace denies `unsafe_code`, its MSRV is 1.85, and it must build
on Windows, macOS (x86_64 + arm64) and Linux.

PLAN §4 proposed Bergamot through a C++ FFI (`cxx`), with CTranslate2 (`ct2rs`) as the
fallback. During the spike a third option turned up: `fxtranslate`, a pure-Rust port of the
Bergamot inference engine that runs the same Mozilla model files.

### Models available for Swedish

| | Mozilla Firefox Translations (Bergamot format) | OPUS-MT (Helsinki-NLP), for CTranslate2 |
|---|---|---|
| sv→en / en→sv | `sv-en` / `en-sv` "tiny" students, release status "Release", run `spring-2024` | `opus-mt-sv-en` / `opus-mt-en-sv`, `opus-2020-02-26` |
| Format | int8 `intgemm.alphas` Marian binary + SentencePiece vocab + lexical shortlist, gzip | Marian `.npz` (float32, 292 MB); must be converted with the CTranslate2 Python converter |
| Hosting | `storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data` (https, Mozilla's registry `db/models.json` lists paths and uncompressed SHA-256) | `object.pouta.csc.fi` zips; converted CTranslate2 files are hosted by nobody, so we'd host them |
| Download, both directions | **30.5 MB** (15.4 + 15.1 MB gzip) | 2 × 272 MB zips → int8 conversion **≈ 162 MB** (80.9 MB per direction) |
| On disk | 30.5 MB (kept gzipped; 44.5 MB unpacked) | ≈ 162 MB |
| License | **MPL-2.0** (repository license of `mozilla/firefox-translations-models`) | **CC-BY-4.0** (LICENSE in the zip); the Hugging Face cards say Apache-2.0 |
| Published quality | flores200-plus COMET22 0.886 (sv→en), 0.892 (en→sv) | Tatoeba BLEU 64.5 / 60.1 (different test set; not comparable) |

## Options

| Option | Pros | Cons | Measurements (this machine) |
|---|---|---|---|
| A. Bergamot C++ (`bergamot-translator`, now `mozilla/translations/inference`) via `cxx` | The engine Firefox ships (as WASM); intgemm/AVX2 int8 kernels; Mozilla models | CMake + C++17 build of marian-fork with 5+ git submodules (intgemm, ruy, sentencepiece, ssplit-cpp, simd_utils); x86 needs MKL (ISSL license, not allowlisted) or OpenBLAS, unless ruy SGEMM is forced; Mozilla's native build script is Linux/Docker only; our `cxx` bridge needs `unsafe extern "C++"`, so an `unsafe_code` exception | **Not built.** See "Not measured" |
| B. CTranslate2 via `ct2rs` 0.10.3 + OPUS-MT int8 | Mature engine; safe Rust API (`cxx` lives inside `ct2rs`); builds from crates.io with Cargo | CMake + MSVC developer environment needed; ruy is the only backend without extra system libraries or an off-allowlist license, and it's slow on x86; models 162 MB (fails SIZE-05), CC-BY-4.0 (owner decision), self-hosted after conversion; changed amounts (`1 234,50 kr` → `1,245 SEK`) | Build 60 s clean; binary 2.7 MB; sv→en short 26.5 ms, paragraph 199 ms (4 threads); 200 MB RSS with both loaded |
| C. `fxtranslate` 0.5 (pure Rust, MPL-2.0) + Mozilla models | Same model files as A; plain Cargo dependency, no CMake, no `unsafe` in our code; builds with Rust 1.85; small and fast even on its scalar kernel | Young crate (first release 2026-07-05, 0.5.0 on 2026-10-04, one maintainer, ≈ 300 recent downloads); its AVX2 kernel doesn't compile with MSVC, so Windows runs the scalar kernel | Build 7 s clean; binary 0.43 MB; sv→en short 7.4 ms, paragraph 95 ms (1 thread); 68 MB RSS with both loaded |

### Measurements

Windows 11 Pro x64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 31 GB RAM, Rust 1.99.1,
MSVC 14.51 (VS 18 Community, its bundled CMake 4.3 + Ninja). Release builds (`lto = "thin"`,
`codegen-units = 1`). Greedy decoding (beam 1) for both. Latency = 3 warm-up runs, then 30
timed runs; nearest-rank p50/p95. "Short" = `Spara ändringar?` / `Save changes?`; "paragraph"
= the 2-sentence, ≈ 25-word second block of `paragraphs-sv` / `paragraphs-en`;
"ui-dialog" = all 8 blocks of `ui-dialog-sv`. RSS = working set of the benchmark process.
Spike programs lived in the session scratchpad and aren't part of the repo.

| | C: fxtranslate, scalar, 1 thread | B: ct2rs ruy int8, 1 thread | B: ct2rs ruy int8, 4 threads |
|---|---|---|---|
| sv→en short | p50 7.4 ms, p95 7.8 ms | 85.6 / 88.3 ms | 26.5 / 31.4 ms |
| sv→en paragraph | 94.6 / 97.8 ms | 655 / 676 ms | 199 / 212 ms |
| sv→en ui-dialog (8 blocks) | 136 / 139 ms (one block at a time) | 593 / 602 ms (one batch) | 191 / 219 ms (one batch) |
| en→sv short | 8.1 / 8.4 ms | 123 / 126 ms | 37.6 / 42.5 ms |
| en→sv paragraph | 92.3 / 93.3 ms | 649 / 665 ms | 197 / 206 ms |
| Load, per direction | 110–120 ms from the `.gz` files (decompress + parse); 16 ms memory-mapped from unpacked files | 132–135 ms | same |
| RSS, both directions loaded | 68 MB heap (51 MB memory-mapped); 69–71 MB after the runs | 200 MB; 262 MB after the runs | same |
| Binary (benchmark exe, stripped) | 0.43 MB | 2.7 MB (static CRT) | same |
| Clean release build | 7 s (20 s with Rust 1.85) | 60 s (CTranslate2 + sentencepiece via CMake/Ninja, 16 threads) | same |

`fxtranslate`'s `translate_batch` was slower than translating blocks one at a time (236 ms vs
136 ms for ui-dialog), so `pg-bergamot` translates block by block.

**SV-05 without a guard** (`numbers-sv`, sv→en): both engines break preserved tokens. CTranslate2
turned `1 234,50 kr` into `1,245 SEK`, dropped the URL path and translated the domain
(`exempel.se` → `example.se`, also inside the e-mail address). fxtranslate turned the URL into
`https://exem.se/conditions`, added `+46` to the phone number and rendered `kl. 23:59` as
`2. 23:59`. Placeholders such as `{1}` survive the Mozilla model unchanged, which the guard
below relies on.

**Build notes**

- B on Windows needs the MSVC developer environment (`vcvars64`) and `CMAKE_GENERATOR=Ninja`;
  plain `cargo build` finds no C++ compiler for CMake. A long target path fails the
  sentencepiece CMake try-compile (250-character object path limit), so the build needed a
  short target directory. `ct2rs`'s `dnnl` feature builds oneDNN from source (≈ 4 min) but
  doesn't link on MSVC with either CRT (oneDNN uses `/MD`, CTranslate2 `/MT`); `mkl` downloads
  Intel oneMKL (ISSL license); `openblas` needs vcpkg. So ruy is the only practical x86 backend.
- C's `gemmology` SIMD shim fails with MSVC (`cl` ignores `-mavx2`; `std::aligned_alloc` is
  missing). The crate then falls back to its scalar kernel without failing the build; all
  numbers above are for the scalar kernel. `pg-bergamot` doesn't request the shim on Windows.
- Both B and C compile with the workspace lints and need no `unsafe` in our crates.

## Decision

**Option C: the Mozilla Firefox Translations (Bergamot) models, run by `fxtranslate`**, in a new
crate `crates/pg-bergamot` (`BergamotTranslator`, id `bergamot`, `is_local() == true`).

- The model choice decides most requirements: 30.5 MB for both directions (SIZE-05), hosted
  over https by Mozilla with published hashes (MDL-02), MPL-2.0 (already on the allowlist),
  and better published quality. OPUS-MT would need ≈ 162 MB, an owner decision on CC-BY-4.0,
  and hosting converted files ourselves.
- Between the two engines for those models, C needs no CMake/C++ toolchain, no `cxx` bridge and
  no `unsafe` exception, and is already fast enough on Windows with its scalar kernel:
  ui-dialog-sv takes ≈ 136 ms, so with OCR at ≈ 19 ms the projected PERF-01 cycle is far below
  600 ms. A's speed advantage (SIMD int8) isn't needed to meet the targets.
- The model files and manifest are engine-neutral (`engine = "bergamot"`). If `fxtranslate`
  stalls or proves wrong, option A can load the same files behind the same `Translator`
  without a model or manifest change. `fxtranslate` is MPL-2.0, so vendoring a fork is possible.

**How it's wired:**

- `models/manifest.toml` lists `bergamot-sv-en` and `bergamot-en-sv` with the `.gz` URLs,
  sizes and SHA-256 of what is downloaded. Files stay gzipped on disk exactly as verified;
  `pg-bergamot` decompresses them (`flate2`, pure Rust) when it loads them. The uncompressed
  model hashes from Mozilla's registry are checked in the real-model test.
- `BergamotTranslator::load_installed` loads every installed direction at startup, so swapping
  sv→en and en→sv never reloads (SV-01). Translation runs on Tokio's blocking pool;
  `translate_long` splits each block into sentences.
- `pg_translate::TokenGuard` wraps the backend (SV-05). It checks each block with
  `check_preserved`. A block that fails is translated again with URLs, e-mails and number runs
  masked as `{n}`. The tokens are then restored, and a number is reformatted only if reading it
  verbatim in the target language would change its value (`12,5` → `12.5`, `1,234.50` →
  `1 234,50`). If that pass fails too, the block's source text is shown instead of wrong digits.
- In the app, live mode uses this backend. Without the models, Swedish blocks report
  `tr-error-model-missing` and the toolbar shows ⬇, which downloads both directions through
  `pg-models` and swaps the loaded translator into the running pipeline. The "MOCK MT" badge only
  appears if the mock backend is ever wired into live mode. This is an interim UI until the model
  manager and first-run wizard (M1-APP-8).

## Verified on this machine

Automated (`cargo test --workspace`, no network):

- `pg-translate` `guard::tests`: masking, localization, placeholder checks, and the guard's
  retry and fallback with a mock backend that alters amounts, times and URLs
  (`sv05_altered_numbers_are_retried_masked`, `sv05_altered_url_is_restored_verbatim`,
  `sv05_falls_back_to_source_when_no_pass_keeps_tokens`).
- `pg-bergamot`: `bundled_manifest_has_both_swedish_directions` (also SIZE-05 < 100 MB),
  `without_models_every_pair_is_model_missing`, `a_broken_installed_model_is_an_error_not_a_panic`.

Real models, `#[ignore]`d because they download ≈ 30 MB
(`cargo test -p pg-bergamot --release --test real_models -- --ignored --test-threads 1`):

- `mdl02_tr2_swedish_models_download_and_verify`: downloads both models from the manifest URLs
  through `pg-models` (SHA-256 checked) and checks the unpacked model files against Mozilla's
  registry hashes.
- `tr1_tr3_both_directions_loaded_and_translate`: both directions loaded; alternating
  sv→en / en→sv short strings without reloading; ui-dialog-sv in ≈ 137 ms.
- `tr4_sv05_numbers_fixtures_keep_preserved_tokens`: all 10 blocks of `numbers-sv` (sv→en) and
  `numbers-en` (en→sv) pass `check_preserved`. 5 and 4 blocks needed the masked pass, none
  fell back to the source. Example: `Totalt att betala: 1 234,50 kr` → `Total to pay: 1,234.50
  kr`; `Läs villkoren på https://exempel.se/villkor` → `Read the terms and conditions of
  https://exempel.se/villkor`.

Manual, release app (`cargo build -p polyglass-desktop --release --features
tauri/custom-protocol`), reading the lens through UI Automation (`lens-read.ps1`) over a
window with "Vill du spara ändringarna innan du stänger?" and "Your changes have been saved.":

- With no models: the lens showed the model-missing message and the ⬇ button labelled with
  30.5 MB; no "MOCK MT" badge.
- After invoking ⬇: both models downloaded and verified in ≈ 2.5 s into
  `%LOCALAPPDATA%\org.polyglass.desktop\models`, were loaded into the running pipeline, and
  the lens showed "Do you want to save the changes before you close?" with the English line
  unchanged. The first uncached cycle took 31 ms (OCR 8.5 ms + translation 21.5 ms).
- After a restart: both directions loaded from disk in ≈ 0.4 s (off the UI thread) and the
  lens translated immediately. `polyglass.exe` working set 126 MB with both models (WebView2
  processes not counted). The log contained no recognized or translated text (PRIV-07).

## Not measured

- **Option A was not built.** Running CMake on the downloaded `mozilla/translations` source was
  not permitted in the session that wrote this ADR, so there are no build-time, size, latency or
  memory numbers for it. What is listed above comes from reading its CMake files. If the owner
  wants the comparison completed, configure `inference/` with `-DUSE_MKL=OFF -DUSE_RUY_SGEMM=ON
  -DCOMPILE_LIBRARY_ONLY=ON` under `vcvars64` + Ninja and time `translator-cli` with the same
  models.
- macOS (x86_64, arm64) and Linux: neither engine was built or timed there. `fxtranslate`
  compiles its C++17 gemmology shim with clang/gcc (AVX2 on x86_64, i8mm on aarch64) and falls
  back to scalar otherwise; whether arm64 Macs get the SIMD kernel, and how fast scalar is
  there, is unknown.
- CTranslate2 with oneDNN or OpenBLAS (didn't link / not installed), and with the Mozilla models
  (CTranslate2's Marian converter has no SSRU decoder, which the "tiny" students use).
- Translation quality beyond the published scores and the spot checks here (SV-07 needs the eval
  sets, M0-FIX-4). Spot checks show typical small-model slips, e.g. `Lagersaldo` →
  `Warer balance`, `Fakturadatum` → `Rakturadatum`.
- The installer-size impact (SIZE-01): `fxtranslate` adds well under 1 MB to the benchmark
  binary; the app wasn't bundled.

## Consequences

- Translation adds no native build step, no `unsafe` and no new license class (MPL-2.0 was
  already allowed for Bergamot). New dependencies: `fxtranslate` (MPL-2.0; vendors gemmology,
  MIT, and xsimd, BSD-3-Clause) and `flate2` with its pure-Rust backend.
- Windows runs the scalar kernel. If PERF-01 gets tight (e.g. longer pages or slower machines,
  REF-A/REF-B in M1-VER-2), fix the MSVC build of fxtranslate's shim upstream (`/arch:AVX2`,
  `_aligned_malloc`) or try clang-cl.
- `fxtranslate` is young: pin it through `Cargo.lock`, re-run the real-model tests on every
  upgrade, and keep option A as the documented fallback.
- The SV-05 guard is backend-agnostic, so the online backends (M3-*) can reuse it.
- Follow-ups: model manager + first-run wizard (M1-APP-8) replace the toolbar ⬇ button; held-out
  quality checks (SV-07); macOS/Linux timings (M0-SPK-3 stays open for those).
