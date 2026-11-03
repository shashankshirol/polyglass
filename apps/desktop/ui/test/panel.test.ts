// Panel mode rules (MODE-02). Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import { modeButton, modeOf, panelItems } from "../src/panel.ts";
import type { RenderedBlock } from "../src/types.ts";

function block(x: number, y: number, source: string, translation: string | null, extra: Partial<RenderedBlock> = {}): RenderedBlock {
  return {
    rect: { x, y, w: 200, h: 18 },
    line_height: 18,
    source,
    source_lang: "sv",
    translation,
    target_lang: "en",
    passthrough: false,
    dim: false,
    bg: "#ffffff",
    fg: "#000000",
    ...extra,
  };
}

test("mode02 panel lists blocks in reading order", () => {
  // Two columns: the pipeline's reading order is the left column first, although the right
  // column's heading sits higher on screen than the left column's second block.
  const blocks = [
    block(8, 40, "Inställningar", "Settings"),
    block(8, 120, "Spara ändringar", "Save changes"),
    block(260, 20, "Hjälp", "Help"),
  ];
  assert.deepEqual(
    panelItems(blocks, false).map((i) => i.text),
    ["Settings", "Save changes", "Help"],
  );
});

test("mode02 panel shows translations, passthrough text and originals with their language", () => {
  const blocks = [
    block(8, 8, "Avbryt", "Cancel"),
    block(8, 30, "Your changes have been saved", null, { passthrough: true, source_lang: "en" }),
    block(8, 52, "Filen kunde inte öppnas", null, { dim: true }),
  ];
  assert.deepEqual(panelItems(blocks, false), [
    { text: "Cancel", lang: "en", dim: false, untranslated: false },
    { text: "Your changes have been saved", lang: "en", dim: false, untranslated: true },
    { text: "Filen kunde inte öppnas", lang: "sv", dim: true, untranslated: true },
  ]);
  // REN-04: left Alt held.
  assert.deepEqual(
    panelItems(blocks, true).map((i) => [i.text, i.lang]),
    [
      ["Avbryt", "sv"],
      ["Your changes have been saved", "en"],
      ["Filen kunde inte öppnas", "sv"],
    ],
  );
});

test("mode02 mode button names the mode and switches to the other", () => {
  assert.equal(modeOf("none"), "overlay");
  assert.equal(modeOf("above"), "panel");
  const overlay = modeButton("none");
  assert.equal(overlay.text, "Overlay");
  assert.equal(overlay.next, "panel");
  const above = modeButton("above");
  assert.equal(above.next, "overlay");
  assert.match(above.label, /^Mode: Panel, translations listed above the lens/);
});
