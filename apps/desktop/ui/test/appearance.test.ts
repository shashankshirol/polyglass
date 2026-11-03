// LENS-07 appearance rules. Run with `npm test`.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";

import {
  DEFAULT_APPEARANCE,
  FONTS,
  blockBackground,
  chromeColors,
  contrast,
  cssVariables,
} from "../src/appearance.ts";
import { MIN_FONT_PX, fitFontSize, minSize, startSize } from "../src/fit.ts";

const hex = (r: number, g: number, b: number): string =>
  `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;

test("lens07 toolbar text stays readable on any frame colour", () => {
  // Every 17th step of each channel: 4,096 colours incl. the greys where both sides are close.
  for (let r = 0; r <= 255; r += 17)
    for (let g = 0; g <= 255; g += 17)
      for (let b = 0; b <= 255; b += 17) {
        const frame = hex(r, g, b);
        const c = chromeColors(frame);
        assert.ok(contrast(frame, c.text) >= 4.5, `${frame}: text ${c.text}`);
        for (const icon of [c.accent, c.ok, c.warn, c.focus]) {
          assert.ok(contrast(frame, icon) >= 3, `${frame}: icon ${icon}`);
        }
      }
});

test("lens07 the default frame keeps the stylesheet's colours", () => {
  const c = chromeColors(DEFAULT_APPEARANCE.frame_color);
  assert.equal(c.chrome, "rgba(24, 44, 66, 0.92)");
  assert.equal(c.text, "#f2f6fa");
  assert.equal(c.accent, "#7fd3ff");
  const css = readFileSync(new URL("../src/style.css", import.meta.url), "utf8");
  for (const [name, value] of Object.entries(cssVariables(DEFAULT_APPEARANCE))) {
    if (name === "--text-font") continue;
    assert.ok(css.includes(`${name}: ${value};`), `style.css default for ${name}`);
  }
  assert.equal(chromeColors("#f0f0f0").text, "#111820", "light frame, dark text");
});

test("lens07 opacity applies to the block background only, within 20-100 %", () => {
  assert.equal(blockBackground("#eceef2", 100), "#eceef2");
  assert.equal(blockBackground("#eceef2", 60), "rgba(236, 238, 242, 0.6)");
  assert.equal(blockBackground("#000000", 5), "rgba(0, 0, 0, 0.2)");
  assert.equal(blockBackground("red", 60), null);
});

test("lens07 font scale scales the start and the smallest size of the fit", () => {
  assert.equal(minSize(1), MIN_FONT_PX);
  assert.equal(minSize(2), 18);
  assert.equal(minSize(0.75), 7);
  assert.equal(startSize(14, 1.5), 21);
  assert.equal(startSize(4, 2), 18);
  assert.equal(startSize(300, 0.75), 36);
  // A block that never fits stops at the scaled minimum and grows instead.
  assert.deepEqual(fitFontSize(() => false, startSize(14, 2), minSize(2)), { size: 18, overflow: true });
  assert.deepEqual(fitFontSize((px) => px <= 20, startSize(14, 2), minSize(2)), { size: 20, overflow: false });
});

test("lens07 every font choice names a font stack with a generic fallback", () => {
  for (const [id, f] of Object.entries(FONTS)) {
    assert.match(f.stack, /(sans-serif|serif|monospace)$/, id);
  }
  // Same ids as `appearance::Font` in Rust (kebab-case).
  const rust = readFileSync(new URL("../../src-tauri/src/appearance.rs", import.meta.url), "utf8");
  const variants = /pub enum Font \{([^}]*)\}/.exec(rust)![1]!
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => /^[A-Z]\w*,$/.test(l))
    .map((l) => l.slice(0, -1).replace(/[A-Z]/g, (c, i) => (i ? "-" : "") + c.toLowerCase()));
  assert.deepEqual(variants.sort(), Object.keys(FONTS).sort());
});

test("lens07 every stylesheet turns animations off for reduced motion", () => {
  // The rule in each sheet (or base.css, which the window sheets import) covers anything that
  // animates now or later.
  const dir = new URL("../src/", import.meta.url);
  const sheets = readdirSync(dir).filter((f) => f.endsWith(".css"));
  assert.ok(sheets.length >= 5, sheets.join());
  for (const f of sheets) {
    const css = readFileSync(new URL(f, dir), "utf8");
    const own = /@media \(prefers-reduced-motion: reduce\) \{\s*\* \{\s*transition: none !important;\s*animation: none !important;/.test(css);
    assert.ok(own || css.includes('@import "./base.css"'), `${f} has no reduced-motion rule`);
  }
});
