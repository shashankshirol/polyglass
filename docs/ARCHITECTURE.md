# Architecture

Companion to [PLAN.md §3](../PLAN.md). This describes the code as it exists; PLAN.md describes
the target.

## Crates

```
pg-core ◄── pg-capture ◄──┐
   ▲   ◄── pg-ocr     ◄──┼── pg-pipeline ◄── pg-bench
   │   ◄── pg-translate ◄┘                   (apps/desktop, later)
   └────── pg-models
```

| Crate | Owns | Key items |
|---|---|---|
| `pg-core` | Types shared by everything | `PhysRect`, `LogicalRect::to_physical` (CAP-04), `Frame` (redacted `Debug`), `Line`, `Block::text` (de-hyphenation), `LangTag`, `LangPair`, `SourceLang`, `Error` |
| `pg-capture` | Getting pixels | `ScreenCapturer`, `StaticCapturer`, `FixtureCapturer` |
| `pg-ocr` | Pixels → lines | `OcrEngine`, `default_engine` (OCR-01 table), `resolve_language` (OCR-06/SV-02), `MockOcr` |
| `pg-translate` | Text → text | `Translator`, `TranslateError` (TR-06 kinds + Fluent IDs), `langid` (SV-04/TR-07), `tokens` (SV-05), `PivotTranslator`/`plan_route` (TR-03), `MockTranslator` |
| `pg-pipeline` | Orchestration | `Scheduler` (CAP-03, LENS-06), `change` (CAP-03/06), `grouping` (OCR-02), `cache` (TR-04), `color` (REN-01), `Pipeline::run_cycle`, `driver::spawn`, `LensUpdate` |
| `pg-models` | Model files | `Manifest` (MDL-01/02), `ModelStore` (MDL-04/05), `Downloader` (MDL-03) |
| `pg-bench` | Measurement | CLI over `Pipeline::run_cycle` (SPEC §5) |

## One cycle

`Pipeline::run_cycle(tick)`:

1. **Blocking pool:** capture `tick.region` → `FrameSignature` → skip if unchanged and not
   forced → "No text found" if blank → OCR (Auto mode prefers the Swedish recognizer, falls back
   to English with a notice) → confidence filter → group lines into blocks → sample bg/fg colour.
2. **Async:** for each block decide its language (fixed, or `langid::decide_sv_en`); blocks
   already in the target language pass through (TR-07); look up the LRU cache; send the misses
   in one request per language pair (TR-05); fill the cache.
3. Return a `LensUpdate` with lens-local logical rects, colours, backend info (PRIV-05) and
   stage timings (`tracing` target `pg::timings`).

## The live driver

`driver::spawn` runs a `tokio::select!` loop over: lens commands, the in-flight cycle, and the
scheduler deadline. Any command that bumps the generation drops the in-flight cycle, and
results are compared with the current generation before being emitted, so results for an old
lens position are never drawn. When hidden, the scheduler has no deadline and the loop only
waits on its channel (LENS-06, RES-01).

## Privacy invariants in code

- `Frame` and `RenderedBlock` have hand-written `Debug` impls without pixels/text (PRIV-02/07).
- `TranslationCache` has no serialization (TR-04: never on disk).
- Errors carry technical context only, never recognized text.
- Model downloads accept only manifest entries; the manifest accepts only https URLs with
  SHA-256 (MDL-02, SEC-03).
