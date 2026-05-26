import { defineConfig } from "@playwright/test";

// Nested config (apps/web is NOT a workspace package), discovered from the
// project root. webServer.command paths must resolve relative to THIS config's
// directory (apps/web), not the project root. `scripts/server.ts` lives at
// apps/web/scripts/server.ts.
export default defineConfig({
  testDir: "./e2e",
  webServer: {
    command: "tsx scripts/server.ts",
    url: "http://localhost:4000",
  },
});
