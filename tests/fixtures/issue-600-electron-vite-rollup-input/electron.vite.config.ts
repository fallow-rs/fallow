import { resolve } from "node:path";
import { defineConfig } from "electron-vite";

export default defineConfig({
  main: {
    build: { rollupOptions: { input: resolve(__dirname, "src/main/index.ts") } },
  },
  preload: {
    // Declared outside the static src/preload/** globs to prove config parsing
    // (not the static fallback) seeds this entry.
    build: { rollupOptions: { input: { bridge: resolve(__dirname, "electron/preload-bridge.ts") } } },
  },
  renderer: {
    build: {
      rollupOptions: {
        input: {
          index: resolve(__dirname, "src/renderer/index.html"),
          settings: resolve(__dirname, "src/renderer/settings/index.html"),
        },
      },
    },
  },
});
