import { defineConfig } from '@playwright/test';

export default defineConfig({
  projects: [{ testDir: './a', testMatch: '*.pw.ts' }, { testDir: './b', testMatch: '*.pw.ts' }],
});
