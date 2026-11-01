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

- `lang` is the fixture's language; `mixed-sv-en` has none (its blocks carry `lang`) and is read
  with the Swedish recognizer, as Auto mode does.
- `image` and `bbox` are reserved for committed images (owner decision D-7 in CHECKLIST.md) and
  are unused today: images are rendered from the text (below), with exact line boxes.
- `mixed-sv-en` blocks carry the expected language (`lang`) and `short: true` for blocks under 20
  characters (SV-04).
- `numbers-en` is a block-by-block translation of `numbers-sv` (SV-05 pairwise test).

| Fixture | Used by |
|---|---|
| `ui-dialog-sv`, `ui-dialog-en` | PERF-01, PERF-06, `pg-bench`; CAP-04 fixture tests (`ui-dialog-en`) |
| `mixed-sv-en` | SV-04 (`pg-translate` langid tests) |
| `numbers-sv`, `numbers-en` | SV-05 (`pg-translate` tokens tests) |
| `paragraphs-sv`, `paragraphs-en` | OCR-02 block grouping (not wired yet) |
| `long-compounds-en` | SV-06, REN-02 |
| all of them | OCR-04 (English set) and SV-03 (Swedish set: `*-sv` and `mixed-sv-en`) accuracy gates, `crates/pg-ocr/tests/accuracy.rs` |

## Images (M0-FIX-2)

`pg_testkit::fixture::FixtureImage::render` draws each `truth.json` with the system UI font:

- at 100%, 125%, 150% and 200% scale, in a light theme (`#F3F3F3` background, `#1B1B1B` text)
  and a dark one (`#202020`, white), the Windows 11 defaults;
- on a page 600 logical px wide (the SPEC §5 lens width) with a 12 px margin, as tall as the text;
- block *i* at a font size (em, as in CSS) of 12, 13, 14, 15 or 16 logical px (`i % 5`),
  word-wrapped, with a gap of 0.8 line heights after each block. Logical px × scale = physical
  px; line tops are whole pixels;
- grayscale anti-aliasing with `ab_glyph`, no hinting and no ClearType, so the images are
  smoother than real screen text. `crates/pg-capture/tests/cleartype.rs` draws the same layout
  with GDI ClearType in real windows instead.

The renderer also returns each line's text and box, which are the ground truth for the CER
scores (`pg_testkit::metrics`) and for mock OCR in `pg-bench`.

**Rendered at test time, not committed.** Tests and `pg-bench` render the images in memory
(a few ms each) and load them through `FixtureCapturer::from_png`. Reasons:

- The font is the OS's own UI font, which is not ours to redistribute, and it is unclear whether
  pictures of its glyphs can be released under CC0 (see the table). Committing images needs an
  owner decision (D-7).
- Each OS renders with the font its users actually see, so the OCR engine on that OS is measured
  on its own UI font.
- Nothing to keep in sync: changing a `truth.json` or the layout changes the images.

The output is deterministic: the same font file gives byte-identical PNGs
(`rendering_is_deterministic`). To look at the images or compare them between machines:

```
cargo run -p pg-testkit --bin render-fixtures -- [OUT_DIR] [FIXTURE...]
```

This writes `OUT_DIR/<fixture>/<fixture>@<scale>-<theme>.png` (default OUT_DIR
`target/fixture-images`; 64 PNGs, ≈ 3 MB with the `.json` files) with a `.json` next to each holding the font path,
the font's and the PNG's SHA-256, and the line boxes, and prints the PNG hashes.

### Fonts

The first of these that exists is used (`pg_testkit::TextRenderer::system`):

| OS | Font file | License | Redistributable? |
|---|---|---|---|
| Windows | `%WINDIR%\Fonts\segoeui.ttf` (Segoe UI; v5.72, SHA-256 `704eada1…0f6e` on the dev machine) | Microsoft, licensed with Windows | No (font file); CC0 release of renderings unclear → D-7 |
| Windows | `%WINDIR%\Fonts\arial.ttf` (fallback) | Monotype, licensed with Windows | No; same question |
| macOS | `/System/Library/Fonts/Supplemental/Arial.ttf`, `/Library/Fonts/Arial.ttf` | Monotype, licensed with macOS | No; same question |
| Linux | DejaVu Sans (`/usr/share/fonts/…/DejaVuSans.ttf`) | Bitstream Vera / DejaVu license (permissive) | Yes |

No font file and no rendering is in the repository.
