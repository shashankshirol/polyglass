// OSS-03: PRIVACY.md is linked from Settings. The button calls a command that takes no input
// and that only the Settings window may call.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const read = (path: string): string => readFileSync(new URL(path, import.meta.url), "utf8");

test("oss03 Settings has a privacy policy button wired to its command", () => {
  assert.match(read("../settings.html"), /<button id="open-privacy" type="button">Privacy policy…<\/button>/);
  assert.match(read("../src/settings.ts"), /\$\("open-privacy"\)\.addEventListener\("click", \(\) => void call\("open_privacy_policy"\)\)/);
});

test("oss03 only the Settings window may open the privacy policy", () => {
  const allowed = (file: string): string[] =>
    (JSON.parse(read(`../../src-tauri/capabilities/${file}`)) as { permissions: string[] }).permissions;
  assert.ok(allowed("settings.json").includes("allow-open-privacy-policy"));
  for (const other of ["lens.json", "models.json", "freeze.json"]) {
    assert.ok(!allowed(other).includes("allow-open-privacy-policy"), other);
  }
  assert.match(read("../../src-tauri/build.rs"), /"open_privacy_policy",/);
});
