import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '../script',
  testMatch: '*.pw.ts',
  globalSetup: './script-setup.ts',
  reporter: '../reporters/custom.ts',
});
