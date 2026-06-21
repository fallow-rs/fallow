import { resolve } from "node:path";
import { defineConfig, externalizeDepsPlugin } from "electron-vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  main: { plugins: [externalizeDepsPlugin()] },
  preload: { plugins: [externalizeDepsPlugin()] },
  // React Compiler (auto-memoization, as codiff does) + Tailwind v4 + `@` alias.
  renderer: {
    resolve: { alias: { "@": resolve(__dirname, "src/renderer/src") } },
    plugins: [react({ babel: { plugins: ["babel-plugin-react-compiler"] } }), tailwindcss()],
  },
});
