// REN-02 fitting rules. Run with `npm test` (Node's test runner strips the types).
import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_FONT_PX, MIN_FONT_PX, fitFontSize, maxHeight, startSize } from "../src/fit.ts";

/** A layout stand-in: text of `chars` characters, 0.5 em wide each, wrapped in a w×h box. */
function box(chars: number, w: number, h: number): (size: number) => boolean {
  return (size) => {
    const perLine = Math.max(1, Math.floor(w / (size * 0.5)));
    const lines = Math.ceil(chars / perLine);
    return lines * size * 1.2 <= h;
  };
}

test("starts at the source text's size, within 9..48 px", () => {
  assert.equal(startSize(14.4), 14);
  assert.equal(startSize(4), MIN_FONT_PX);
  assert.equal(startSize(300), MAX_FONT_PX);
  assert.equal(startSize(Number.NaN), MIN_FONT_PX);
});

test("text that fits keeps the starting size", () => {
  assert.deepEqual(fitFontSize(box(10, 200, 20), 14), { size: 14, overflow: false });
});

test("longer text shrinks to the largest size that fits", () => {
  // 40 chars in 200×20: one line needs size ≤ 10 (200 / 20 chars); 1.2 × 10 ≤ 20.
  const fit = fitFontSize(box(40, 200, 20), 16);
  assert.deepEqual(fit, { size: 10, overflow: false });
  assert.ok(box(40, 200, 20)(fit.size));
  assert.ok(!box(40, 200, 20)(fit.size + 1));
});

test("never goes below 9 px; overflowing text is flagged so the box grows", () => {
  assert.deepEqual(fitFontSize(box(400, 100, 12), 16), { size: MIN_FONT_PX, overflow: true });
  assert.deepEqual(fitFontSize(() => false, 4), { size: MIN_FONT_PX, overflow: true });
});

test("a grown block stops at the interior's bottom edge", () => {
  assert.equal(maxHeight(100, 164), 64);
  assert.equal(maxHeight(200, 164), 0);
});
