# Fixtures

All fixtures are created for this project and released under **CC0-1.0** (SPEC §5).

Each directory holds a `truth.json`:

```json
{
  "fixture": "ui-dialog-sv",
  "license": "CC0-1.0",
  "lang": "sv",
  "image": null,
  "blocks": [{ "text": "Spara ändringar?", "bbox": { "x": 0, "y": 0, "w": 0, "h": 0 } }]
}
```

- `image` is the PNG rendered from the truth (added by the generator, CHECKLIST M0-FIX-2); while
  it is `null`, `pg-bench` synthesizes a frame and lays blocks out top to bottom.
- `bbox` (optional until images exist) is in physical pixels at 100% scale.
- `mixed-sv-en` blocks carry the expected language (`lang`) and `short: true` for blocks under 20
  characters (SV-04).
- `numbers-en` is a block-by-block translation of `numbers-sv` (SV-05 pairwise test).

| Fixture | Used by |
|---|---|
| `ui-dialog-sv`, `ui-dialog-en` | PERF-01, PERF-06, `pg-bench` |
| `mixed-sv-en` | SV-04 (`pg-translate` langid tests) |
| `numbers-sv`, `numbers-en` | SV-05 (`pg-translate` tokens tests) |
| `paragraphs-sv`, `paragraphs-en` | OCR-02 block grouping (needs images/boxes) |
| `long-compounds-en` | SV-06, REN-02 |
