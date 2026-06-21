import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.e2e.ts",
  timeout: 200_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
});
