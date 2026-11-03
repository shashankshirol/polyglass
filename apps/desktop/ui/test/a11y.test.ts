// A11Y-01 checks on the window styles and pages. Run with `npm test`. The live checks (keyboard
// order, names in UI Automation) are in CHECKLIST.md M4-6.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { contrast } from "../src/appearance.ts";

const read = (path: string): string => readFileSync(new URL(path, import.meta.url), "utf8");

/** The colour tokens of base.css: the light `:root` and the dark one. */
function themes(): Record<string, string>[] {
  const css = read("../src/base.css");
  return [...css.matchAll(/:root \{([^}]*)\}/g)].map((m) => {
    const tokens = Object.fromEntries([...m[1]!.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})/gi)].map((t) => [t[1]!, t[2]!]));
    return tokens;
  });
}

test("a11y01 Settings and model manager colours meet WCAG 2.1 AA in light and dark", () => {
  const all = themes();
  assert.equal(all.length, 2, "a light and a dark :root");
  const [light, dark] = all as [Record<string, string>, Record<string, string>];
  for (const [name, t] of [
    ["light", light],
    ["dark", { ...light, ...dark }],
  ] as const) {
    // Text (1.4.3): 4.5:1.
    for (const [fg, bg] of [
      ["fg", "bg"],
      ["fg", "card"],
      ["muted", "bg"],
      ["muted", "card"],
      ["accent-fg", "accent"],
      ["ok", "card"],
      ["warn", "card"],
      ["warn", "warn-bg"],
      ["fg", "warn-bg"],
      ["danger", "card"],
      ["fg", "accent-soft"],
    ]) {
      const r = contrast(t[fg!]!, t[bg!]!);
      assert.ok(r >= 4.5, `${name}: --${fg} on --${bg} is ${r.toFixed(2)}:1`);
    }
    // Focus ring and the borders that show a text field or list (1.4.11): 3:1.
    for (const [fg, bg] of [
      ["focus", "bg"],
      ["focus", "card"],
      ["focus", "accent-soft"],
      ["control-line", "bg"],
      ["control-line", "card"],
    ]) {
      const r = contrast(t[fg!]!, t[bg!]!);
      assert.ok(r >= 3, `${name}: --${fg} on --${bg} is ${r.toFixed(2)}:1`);
    }
  }
});

test("a11y01 text fields and lists draw their border in the control colour", () => {
  assert.match(read("../src/settings.css"), /select \{[^}]*border: 1px solid var\(--control-line\)/);
  assert.match(read("../src/models.css"), /\.import-row input \{[^}]*border: 1px solid var\(--control-line\)/);
  // Every focusable thing in the windows shows a focus ring.
  assert.match(read("../src/base.css"), /button:focus-visible,\s*input:focus-visible,\s*select:focus-visible,\s*summary:focus-visible,\s*code:focus-visible \{\s*outline: 2px solid var\(--focus\)/);
});

test("a11y01 every control in the pages has a name", () => {
  for (const page of ["../index.html", "../settings.html", "../models.html", "../freeze.html"]) {
    const html = read(page);
    for (const m of html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)) {
      const attrs = m[1]!;
      const text = m[2]!.replace(/<[^>]+>/g, "").trim();
      assert.ok(/aria-label="[^"]+"/.test(attrs) || text.length > 0, `${page}: unnamed button ${attrs}`);
    }
    for (const m of html.matchAll(/<(input|select)\b([^>]*)>/g)) {
      const attrs = m[2]!;
      const id = /id="([^"]+)"/.exec(attrs)?.[1];
      const labelled = /aria-label="[^"]+"/.test(attrs) || (id !== undefined && html.includes(`<label for="${id}"`));
      assert.ok(labelled, `${page}: unnamed ${m[1]} ${attrs}`);
    }
  }
});
