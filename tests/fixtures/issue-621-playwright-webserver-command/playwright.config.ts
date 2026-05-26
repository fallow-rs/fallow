import { defineConfig } from "@playwright/test";

// `srvx` is a CLI-only dependency invoked here, never imported in source.
// `scripts/e2e-server.ts` is launched via tsx and is not imported anywhere either.
export default defineConfig({
  testDir: "./e2e",
  webServer: [
    {
      command: "srvx --port 3000",
      url: "http://localhost:3000",
    },
    {
      command: "tsx scripts/e2e-server.ts",
      url: "http://localhost:4000",
    },
  ],
});
