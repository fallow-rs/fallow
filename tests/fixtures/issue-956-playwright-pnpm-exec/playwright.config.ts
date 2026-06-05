import { defineConfig, devices } from "@playwright/test";

const PORT = 3000;

export default defineConfig({
  testDir: "./e2e",
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: `pnpm build && pnpm exec srvx --prod --port ${PORT} --hostname 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}/`,
  },
});
