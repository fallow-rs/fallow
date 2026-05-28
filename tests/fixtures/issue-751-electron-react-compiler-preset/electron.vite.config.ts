import { defineConfig } from "electron-vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import babel from "@rolldown/plugin-babel";

export default defineConfig({
  main: { build: { rollupOptions: { input: "src/main/index.ts" } } },
  renderer: {
    plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
  },
});
