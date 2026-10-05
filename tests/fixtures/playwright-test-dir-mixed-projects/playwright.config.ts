import { defineConfig } from '@playwright/test';

export default defineConfig({
  projects: [{ testDir: './a', testMatch: '*.pw.ts' }, { name: 'chromium' }],
});
