# Translation evaluation sets (SV-07)

Written for this project and released under **CC0-1.0**, like the fixtures.

| Set | Direction | Segments | References |
|---|---|---|---|
| `sv-en` | Swedish → English | 200: 80 UI, 60 web, 60 document | US English (SV-10) |
| `en-sv` | English → Swedish | 200: 80 UI, 60 web, 60 document | Swedish |

The two sets were written separately: `en-sv`'s English sources are not `sv-en`'s references
(test `the_sets_are_written_separately`), so neither direction is scored on the other's output.

- **UI**: buttons, menus, dialogs, settings, errors and status lines, as the lens sees them.
- **Web**: shop, news, booking, travel, job and form text, cookie banners.
- **Document**: e-mails and letters, reports, contracts, manuals, notices from authorities,
  schools and healthcare, and a little prose.

Segments carry numbers, dates, times, prices, error codes and compounds on purpose, as UI text
does. A reference is one good translation, not the only one; chrF rewards overlap with it, so
compare scores between backends or versions on the same set rather than reading them as absolute
quality.

## Format

`<set>/segments.json`:

```json
{
  "set": "sv-en",
  "license": "CC0-1.0",
  "source": "sv",
  "target": "en",
  "description": "…",
  "segments": [{ "id": 1, "domain": "ui", "source": "Spara", "reference": "Save" }]
}
```

Ids run 1–200 in file order. Text is NFC with single spaces (test
`sv07_both_sets_have_200_clean_segments` in `pg-testkit`, which loads them through
`pg_testkit::eval`).

## Scoring

```text
cargo run --release -p pg-bench -- --eval tests/eval/sv-en --translator ct2
cargo run --release -p pg-bench -- --eval tests/eval/en-sv --translator ct2 --hypotheses en-sv.json
```

`pg-bench --eval` sends each segment to the backend as one block, the way the lens sends a block,
and prints corpus chrF overall and per domain plus each segment's translation latency.
`--hypotheses` writes every output with its sentence chrF for review. chrF is
`pg_testkit::metrics::chrf_stats`: sacreBLEU's default chrF (character 6-grams, β = 2, whitespace
removed, signature `nrefs:1|case:mixed|eff:yes|nc:6|nw:0|space:no`); it gave the same corpus and
sentence scores as sacreBLEU 2.5.1 on the OPUS-MT outputs. `--translator ct2` loads the models
from the app's store (`--models <dir>` for another one) and wraps them in the SV-05 token guard as
the app does; `--no-guard` leaves the guard out.

The sources double as a held-out set for the Auto-mode language identification (SV-04, test
`sv04_held_out_eval_sources` in `pg-translate`).

## Not done yet

- A native Swedish speaker hasn't reviewed the Swedish sources and references.
- SV-07's per-release chrF report and the native-speaker rating of 50 outputs per direction are
  M5-6.
