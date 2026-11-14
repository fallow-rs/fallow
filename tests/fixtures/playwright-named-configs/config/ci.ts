import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '../script',
  testMatch: '*.pw.ts',
  globalSetup: './ci-setup.ts',
});
