// The toolbar's language pickers and the lens texts that name languages (MDL-07, SV-02). Pure
// functions, tested with `npm test`. Rust validates every choice against the model catalog;
// this only decides what to offer and how to word it. No language is named here: names come
// from the platform's `Intl.DisplayNames`, so a pair added to the catalog needs no UI change.

import type { LanguagesView } from "./types";

const NAMES = (() => {
  try {
    return new Intl.DisplayNames(["en"], { type: "language", fallback: "none" });
  } catch {
    return null;
  }
})();

/** English name of a language tag (`de` → German), or the tag itself if unknown. */
export function langName(tag: string): string {
  return NAMES?.of(tag) ?? tag;
}

export interface PickerOption {
  /** `auto:<source>` / `fixed:<source>` for the source picker, the tag for the target picker. */
  value: string;
  text: string;
  /** The model isn't installed yet. */
  missing: boolean;
}

const MISSING_MARK = " ⬇";

function installed(v: LanguagesView, source: string, target: string): boolean {
  return v.pairs.some((p) => p.source === source && p.target === target && p.installed);
}

/** Sources the catalog has for the current target, each as Auto (source/target) and fixed. */
export function sourceOptions(v: LanguagesView): PickerOption[] {
  const out: PickerOption[] = [];
  for (const p of v.pairs.filter((p) => p.target === v.target)) {
    const missing = !installed(v, p.source, p.target);
    const mark = missing ? MISSING_MARK : "";
    out.push({
      value: `auto:${p.source}`,
      text: `${langName(p.source)}/${langName(p.target)}${mark}`,
      missing,
    });
    out.push({ value: `fixed:${p.source}`, text: `${langName(p.source)} only${mark}`, missing });
  }
  return out;
}

/** Every target the catalog has, by name. */
export function targetOptions(v: LanguagesView): PickerOption[] {
  const targets = [...new Set(v.pairs.map((p) => p.target))];
  return targets
    .map((t) => ({
      value: t,
      text: langName(t),
      missing: !v.pairs.some((p) => p.target === t && p.installed),
    }))
    .sort((a, b) => a.text.localeCompare(b.text));
}

export function sourceValue(v: LanguagesView): string {
  return `${v.auto ? "auto" : "fixed"}:${v.source}`;
}

/** The source to use with a new target: the current one if the catalog has that pair, else
 * the old target (a swap), else the first source the catalog has for the new target. */
export function pickSource(v: LanguagesView, target: string): string | null {
  const has = (s: string): boolean => v.pairs.some((p) => p.source === s && p.target === target);
  if (has(v.source)) return v.source;
  if (has(v.target)) return v.target;
  return v.pairs.find((p) => p.target === target)?.source ?? null;
}

/** `{ source, target, auto }` for `set_languages` from a source-picker value. */
export function parseSourceValue(value: string): { source: string; auto: boolean } | null {
  const m = /^(auto|fixed):([A-Za-z0-9-]{2,32})$/.exec(value);
  return m ? { source: m[2]!, auto: m[1] === "auto" } : null;
}

export function sourceLabel(v: LanguagesView): string {
  const s = langName(v.source);
  return v.auto
    ? `Source language: ${s} or ${langName(v.target)}, detected per block`
    : `Source language: ${s}`;
}

/** The model-missing message for the selected pair (`tr-error-model-missing` and its import
 * and unreachable variants). `command`: the install command when downloading isn't possible. */
export function modelMessage(
  v: LanguagesView,
  kind: "download" | "import" | "unreachable",
  missingBytes: number,
  command: string,
): string {
  const both = v.canSwap;
  const pair = `${langName(v.source)} ${both ? "↔" : "→"} ${langName(v.target)}`;
  const what = both ? `The ${pair} translation models aren't` : `The ${pair} translation model isn't`;
  const size = `about ${Math.max(1, Math.round(missingBytes / 1e6))} MB`;
  const install = `Install ${both ? "them" : "it"} once from a terminal in the Polyglass repository with: ${command} (README → First run), then restart Polyglass.`;
  switch (kind) {
    case "download":
      return `${what} downloaded yet. Click ⬇ in the toolbar to download (${size}, once).`;
    case "import":
      return `${what} installed yet. ${install}`;
    case "unreachable":
      return `${what} downloadable: their release isn't reachable (the Polyglass repository may be private). ${install}`;
  }
}

/** SV-02 (any source language): where to add the missing OCR language on Windows. `fallback`:
 * Auto mode still reads the target language. */
export function ocrHelp(lang: string, target: string, fallback: boolean): string {
  const name = langName(lang);
  const reads = fallback ? `so only ${langName(target)} is read` : "so this text can't be read";
  return `${name} text recognition isn't installed, ${reads}. Add it in Windows Settings → Time & language → Language & region → ${name} → Language options → Optical character recognition (⚠ ${primary(lang).toUpperCase()} in the toolbar opens Settings).`;
}

export function primary(tag: string): string {
  return tag.split("-")[0] ?? tag;
}
