import { defineConfig } from "@rstest/core";

export default defineConfig({
  coverage: { provider: "v8" },
  projects: [
    "packages/*",
    "!packages/excluded",
    {
      name: "unit",
      root: "src",
      include: ["**/*.check.ts", "!**/ignored.check.ts"],
      exclude: { patterns: ["skipped.check.ts"] },
      setupFiles: "./setup/per-file.ts",
      globalSetup: "<rootDir>/setup/global.ts",
      resolve: { alias: { "@theme": "./dom/theme" } },
    },
    {
      name: "dom",
      root: "dom",
      include: ["**/*.dom.ts"],
      testEnvironment: "jsdom",
      browser: { enabled: true },
    },
  ],
});
