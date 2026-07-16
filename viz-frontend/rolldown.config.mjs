import { defineConfig } from "rolldown";

export default defineConfig({
  input: "src/main.ts",
  output: {
    file: "../crates/cli/viz-assets/viz.js",
    format: "iife",
    minify: true,
  },
});
