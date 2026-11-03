import { defineConfig } from "vite";

// The lens UI is plain TypeScript + DOM (PLAN §4: no UI framework). Tauri loads the dev server
// in `tauri dev` and the bundled `dist/` otherwise (SEC-02: bundled assets only).
export default defineConfig({
  root: "ui",
  clearScreen: false,
  server: {
    host: "localhost",
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
    // Four pages: the lens, the model manager / first-run wizard window, the Freeze mode
    // snapshot window and Settings. Paths are relative to this folder, where npm runs.
    rollupOptions: {
      input: ["ui/index.html", "ui/models.html", "ui/freeze.html", "ui/settings.html"],
    },
  },
});
