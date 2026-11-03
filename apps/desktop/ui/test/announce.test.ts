// Screen-reader announcements (REN-06). Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import { Announcer, type Spoken } from "../src/announce.ts";

const en = (text: string): Spoken => ({ text, lang: "en" });

test("ren06 announces new text at most once every 2 s", () => {
  const a = new Announcer();
  assert.deepEqual(a.offer([en("Save changes")], 0).announce, [en("Save changes")]);
  // 500 ms later: waits for the rest of the gap.
  const r = a.offer([en("Cancel")], 500);
  assert.equal(r.announce, null);
  assert.equal(r.waitMs, 1500);
  // A newer update replaces the waiting one.
  assert.equal(a.offer([en("Open folder")], 900).waitMs, 1100);
  assert.equal(a.flush(1999), null, "too early");
  assert.deepEqual(a.flush(2000), [en("Open folder")]);
  assert.equal(a.flush(2100), null, "said once");
});

test("ren06 does not repeat what was announced, and repeats after a reset", () => {
  const a = new Announcer();
  const blocks = [en("Do you want to save the changes before closing?"), { text: "Spara", lang: "sv" }];
  assert.notEqual(a.offer(blocks, 0).announce, null);
  assert.deepEqual(a.offer(blocks, 5000), { announce: null, waitMs: null });
  // New text is announced, the next waits for the gap, then the lens is back on the announced
  // text: the waiting one is dropped.
  assert.deepEqual(a.offer([en("Cancel")], 5100).announce, [en("Cancel")]);
  assert.equal(a.offer([en("Open folder")], 5200).waitMs, 1900);
  assert.deepEqual(a.offer([en("Cancel")], 5300), { announce: null, waitMs: null });
  assert.equal(a.flush(9000), null);
  // Nothing on screen announces nothing.
  assert.deepEqual(a.offer([], 9000), { announce: null, waitMs: null });
  a.reset();
  assert.deepEqual(a.offer(blocks, 9100).announce, blocks);
});
