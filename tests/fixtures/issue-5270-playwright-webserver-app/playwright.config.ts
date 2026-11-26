import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  webServer: {
    command: 'npm start',
    url: 'http://localhost:3000',
  },
});
