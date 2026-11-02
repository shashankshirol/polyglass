// Model manager and wizard rules (MDL-01/03/05, SV-01). Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  formatBytes,
  pairName,
  percent,
  rowActions,
  rowState,
  selectedBytes,
  wizardStage,
} from "../src/catalog.ts";
import type { ModelRow, ModelsView } from "../src/types.ts";

const MB = 1e6;

function row(id: string, extra: Partial<ModelRow> = {}): ModelRow {
  return {
    id,
    source: "sv",
    target: "en",
    version: "1",
    download_size: 84.8 * MB,
    downloadable: true,
    license: "CC-BY-4.0",
    attribution: "OPUS-MT by Helsinki-NLP, CC-BY 4.0",
    source_url: "https://example.org",
    installed: false,
    on_disk: 0,
    suggested: true,
    ...extra,
  };
}

function view(extra: Partial<ModelsView> = {}): ModelsView {
  return {
    rows: [row("mt-sv-en"), row("mt-en-sv")],
    disk_usage: 0,
    store_path: "C:\\models",
    job: null,
    message_id: null,
    unreachable: false,
    first_run: true,
    install_command: "python scripts/models/opus_mt.py",
    wanted_ids: ["mt-sv-en", "mt-en-sv"],
    wanted_missing_bytes: 169.6 * MB,
    wanted_downloadable: true,
    ...extra,
  };
}

test("sizes and names", () => {
  assert.equal(formatBytes(169_662_336), "169.7 MB");
  assert.equal(formatBytes(0), "0.0 MB");
  assert.equal(formatBytes(1.5e9), "1.50 GB");
  assert.equal(pairName({ source: "sv", target: "en" }), "Swedish → English");
  assert.equal(pairName({ source: "fi", target: "en" }), "Finnish → English");
  assert.equal(percent({ ids: [], downloaded: 50, total: 200 }), 25);
  assert.equal(percent({ ids: [], downloaded: 5, total: 0 }), 100);
});

test("MDL-03/05: each row offers what its state allows", () => {
  const v = view();
  assert.deepEqual(rowActions(row("a"), v), { download: "Download", remove: null, installCommand: false });
  assert.deepEqual(rowActions(row("a", { on_disk: 10 }), v), {
    download: "Resume",
    remove: "Discard",
    installCommand: false,
  });
  assert.deepEqual(rowActions(row("a", { installed: true, on_disk: 9 }), v), {
    download: null,
    remove: "Delete",
    installCommand: false,
  });
  const running = view({ job: { ids: ["a"], downloaded: 1, total: 2 } });
  assert.equal(rowState(row("a", { on_disk: 1 }), running), "downloading");
  assert.deepEqual(rowActions(row("a"), running), { download: null, remove: null, installCommand: false });
});

test("no dead download button: import-only or unreachable rows show the install command", () => {
  assert.deepEqual(rowActions(row("a", { downloadable: false }), view()), {
    download: null,
    remove: null,
    installCommand: true,
  });
  assert.deepEqual(rowActions(row("a", { on_disk: 5 }), view({ unreachable: true })), {
    download: null,
    remove: "Discard",
    installCommand: true,
  });
  // Installed models stay deletable either way.
  assert.equal(rowActions(row("a", { installed: true }), view({ unreachable: true })).remove, "Delete");
});

test("SV-01: the wizard shows the combined size of the selected models", () => {
  const rows = [row("sv-en"), row("en-sv"), row("de-en", { installed: true })];
  assert.equal(selectedBytes(rows, new Set(["sv-en", "en-sv"])), 169.6 * MB);
  assert.equal(selectedBytes(rows, new Set(["sv-en"])), 84.8 * MB);
  assert.equal(selectedBytes(rows, new Set(["de-en"])), 0, "installed: nothing to download");
});

test("wizard stages", () => {
  assert.equal(wizardStage(view()), "offer");
  assert.equal(wizardStage(view({ job: { ids: ["a"], downloaded: 0, total: 1 } })), "downloading");
  assert.equal(wizardStage(view({ wanted_missing_bytes: 0 })), "ready");
  assert.equal(wizardStage(view({ wanted_downloadable: false })), "install-command");
});
