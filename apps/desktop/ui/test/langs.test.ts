// Language picker rules and the texts that name languages (MDL-07, SV-02). Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  langName,
  modelMessage,
  ocrHelp,
  parseSourceValue,
  pickSource,
  sourceLabel,
  sourceOptions,
  sourceValue,
  targetOptions,
} from "../src/langs.ts";
import type { LanguagesView } from "../src/types.ts";

function view(extra: Partial<LanguagesView> = {}): LanguagesView {
  return {
    source: "sv",
    target: "en",
    auto: true,
    // Catalog order: Swedish pairs first.
    pairs: [
      { source: "en", target: "sv", installed: true },
      { source: "sv", target: "en", installed: true },
      { source: "de", target: "en", installed: false },
    ],
    canSwap: true,
    ...extra,
  };
}

test("names come from the platform, not a table", () => {
  assert.equal(langName("sv"), "Swedish");
  assert.equal(langName("de"), "German");
  assert.equal(langName("fi"), "Finnish");
  assert.equal(langName("qqq"), "qqq");
});

test("mdl07 source picker offers each catalog source for the target, auto and fixed", () => {
  const opts = sourceOptions(view());
  assert.deepEqual(
    opts.map((o) => [o.value, o.text, o.missing]),
    [
      ["auto:sv", "Swedish/English", false],
      ["fixed:sv", "Swedish only", false],
      ["auto:de", "German/English ⬇", true],
      ["fixed:de", "German only ⬇", true],
    ],
  );
  assert.equal(sourceValue(view()), "auto:sv");
  assert.equal(sourceValue(view({ auto: false })), "fixed:sv");
  // For target Swedish only English is a source.
  assert.deepEqual(
    sourceOptions(view({ source: "en", target: "sv" })).map((o) => o.value),
    ["auto:en", "fixed:en"],
  );
});

test("mdl07 target picker lists every target once, by name", () => {
  assert.deepEqual(
    targetOptions(view()).map((o) => [o.value, o.missing]),
    [
      ["en", false],
      ["sv", false],
    ],
  );
});

test("a new target keeps the source if it can, else swaps, else takes the catalog's", () => {
  assert.equal(pickSource(view(), "sv"), "en", "sv→en becomes en→sv");
  assert.equal(pickSource(view({ source: "de" }), "sv"), "en");
  assert.equal(pickSource(view({ source: "de" }), "en"), "de");
  assert.equal(pickSource(view(), "fi"), null);
});

test("sec01 only well-formed picker values reach set_languages", () => {
  assert.deepEqual(parseSourceValue("auto:de"), { source: "de", auto: true });
  assert.deepEqual(parseSourceValue("fixed:sv-SE"), { source: "sv-SE", auto: false });
  for (const bad of ["", "de", "auto:", "maybe:de", "auto:../x", `auto:${"a".repeat(40)}`]) {
    assert.equal(parseSourceValue(bad), null, bad);
  }
});

test("labels and messages name the selected pair", () => {
  assert.equal(sourceLabel(view()), "Source language: Swedish or English, detected per block");
  assert.equal(sourceLabel(view({ source: "de", auto: false })), "Source language: German");
  const sv = modelMessage(view(), "download", 169.7e6, "python scripts/models/opus_mt.py");
  assert.match(sv, /^The Swedish ↔ English translation models aren't downloaded yet\./);
  assert.match(sv, /about 170 MB/);
  const de = modelMessage(
    view({ source: "de", canSwap: false }),
    "import",
    85.7e6,
    "python scripts/models/opus_mt.py opus-mt-de-en",
  );
  assert.match(de, /^The German → English translation model isn't installed yet\./);
  assert.match(de, /python scripts\/models\/opus_mt\.py opus-mt-de-en/);
  assert.match(modelMessage(view(), "unreachable", 1, "cmd"), /isn't reachable/);
});

test("sv02 ocr help names the missing language and what still works", () => {
  const auto = ocrHelp("de", "en", true);
  assert.match(auto, /^German text recognition isn't installed, so only English is read\./);
  assert.match(auto, /→ German → Language options/);
  assert.match(auto, /⚠ DE in the toolbar/);
  assert.match(ocrHelp("sv", "en", false), /so this text can't be read/);
});
