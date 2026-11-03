// SV-06 long Swedish compounds: whole words first, then breaking at the box edge. Run with
// `npm test`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MIN_FONT_PX, fitBlock, startSize } from "../src/fit.ts";

/** Average glyph width as a share of the font size (Segoe UI is ≈ 0.5 em for lower case). */
const EM = 0.5;
const LINE = 1.2;

/**
 * A layout stand-in for a block `w` px wide: words fill lines greedily; a word wider than the
 * line overflows it (`breakWords` off) or is cut at the edge (on), like `overflow-wrap: anywhere`.
 * Hyphenation is left out: it only adds break points, so this is the webview without it.
 */
function layout(text: string, size: number, w: number, breakWords: boolean): { lines: number; widest: number } {
  const perLine = Math.max(1, Math.floor(w / (size * EM)));
  let lines = 1;
  let used = 0;
  let widest = 0;
  for (const word of text.split(/\s+/)) {
    let len = word.length;
    const need = used === 0 ? len : len + 1;
    if (used + need <= perLine) {
      used += need;
    } else if (len <= perLine) {
      lines += used === 0 ? 0 : 1;
      used = len;
    } else if (!breakWords) {
      if (used > 0) lines++;
      used = len;
    } else {
      if (used > 0) lines++;
      while (len > perLine) {
        lines++;
        len -= perLine;
      }
      used = len;
    }
    widest = Math.max(widest, used * size * EM);
  }
  return { lines, widest };
}

function fitsIn(text: string, w: number, h: number) {
  return (size: number, breakWords: boolean): boolean => {
    const l = layout(text, size, w, breakWords);
    return l.widest <= w && l.lines * size * LINE <= h;
  };
}

test("sv06 a long compound shrinks before it is broken", () => {
  // "Fordonsregistreringsbevis" (25 chars) in a box made for 20 chars at 14 px: it fits whole at
  // 11 px, which beats breaking it at 14 px over two lines of a two-line box.
  const w = 20 * 14 * EM;
  const fit = fitBlock(fitsIn("Fordonsregistreringsbevis", w, 2 * 14 * LINE), 14);
  assert.deepEqual(fit, { size: 11, overflow: false, breakWords: false });
});

test("sv06 words break at the edge only when no size fits them whole", () => {
  const w = 10 * 14 * EM; // 10 chars at 14 px; the word needs 25 chars even at 9 px (≈ 15).
  const fit = fitBlock(fitsIn("Brandsäkerhetsinspektionskrav", w, 3 * 14 * LINE), 14);
  assert.equal(fit.breakWords, true);
  assert.equal(fit.overflow, false);
  assert.ok(layout("Brandsäkerhetsinspektionskrav", fit.size, w, true).widest <= w);
});

test("sv06 long-compounds-en: no Swedish block overflows its box beyond what REN-02 allows", () => {
  const truth = JSON.parse(
    readFileSync(new URL("../../../../tests/fixtures/long-compounds-en/truth.json", import.meta.url), "utf8"),
  ) as { blocks: { text: string; expected_sv_hint: string }[] };
  assert.equal(truth.blocks.length, 6);
  for (const size of [12, 13, 14, 15, 16]) {
    for (const b of truth.blocks) {
      // The box was made for the English text on one line (the fixture's premise).
      const w = b.text.length * size * EM;
      const h = size * LINE;
      const sv = b.expected_sv_hint;
      const fit = fitBlock(fitsIn(sv, w, h), startSize(h));
      const l = layout(sv, fit.size, w, fit.breakWords);
      // Never wider than the box (SV-06: break at the edge instead of overflowing).
      assert.ok(l.widest <= w, `${sv} at ${size}px: ${l.widest} > ${w}`);
      // Taller than the box only at the smallest size (REN-02: then the box grows downward).
      if (l.lines * fit.size * LINE > h) {
        assert.equal(fit.size, MIN_FONT_PX, `${sv} at ${size}px grew above 9 px`);
        assert.equal(fit.overflow, true);
      }
    }
  }
});

test("sv06 rendered translations carry their language for hyphenation", () => {
  // The lens tags each block and panel item with `lang` (REN-06 and SV-06); the stylesheet asks
  // for hyphenation there and breaks words only on the fallback class.
  const css = readFileSync(new URL("../src/style.css", import.meta.url), "utf8");
  const block = /\n\.block \{[^}]*\}/.exec(css)![0];
  assert.match(block, /hyphens: auto/);
  assert.doesNotMatch(block, /overflow-wrap/);
  assert.match(css, /\.block\.break-words \{\s*overflow-wrap: anywhere;/);
  const main = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  assert.match(main, /el\.lang = translated \? b\.target_lang : b\.source_lang/);
  assert.match(main, /li\.lang = item\.lang/);
});
