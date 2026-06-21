import { defineConfig, externalizeDepsPlugin } from "electron-vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  main: { plugins: [externalizeDepsPlugin()] },
  preload: { plugins: [externalizeDepsPlugin()] },
  // React Compiler (auto-memoization), as codiff does.
  renderer: { plugins: [react({ babel: { plugins: ["babel-plugin-react-compiler"] } })] },
});
