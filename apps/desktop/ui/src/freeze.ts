// Freeze mode window (MODE-03): draws the snapshot Rust took of the monitor, then asks to be
// shown under the lens. The pixels arrive over IPC and live only in this canvas (PRIV-02).

import { invoke } from "@tauri-apps/api/core";
import { decodeSnapshot } from "./snapshot";

const canvas = document.getElementById("snapshot") as HTMLCanvasElement;

async function start(): Promise<void> {
  const image = decodeSnapshot(await invoke<ArrayBuffer>("freeze_snapshot"));
  canvas.width = image.width;
  canvas.height = image.height;
  canvas.getContext("2d")?.putImageData(image, 0, 0);
  await invoke("freeze_ready");
}

// Esc leaves Freeze mode (so do Ctrl+Alt+F and the lens's ❄).
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") void invoke("set_frozen", { frozen: false });
});

start().catch((e: unknown) => {
  console.error("freeze failed", e);
  void invoke("set_frozen", { frozen: false });
});
