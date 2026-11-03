// Freeze mode snapshot framing (MODE-03). Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import { snapshotSize } from "../src/snapshot.ts";

test("mode03 snapshot header gives the size and must match the pixels", () => {
  const buf = new Uint8Array([2, 0, 0, 0, 1, 0, 0, 0, 3, 2, 1, 255, 6, 5, 4, 255]).buffer;
  assert.deepEqual(snapshotSize(buf), { width: 2, height: 1 });
  assert.throws(() => snapshotSize(buf.slice(0, 12)), /mismatch/);
  assert.throws(() => snapshotSize(new ArrayBuffer(4)), /too short/);
});
