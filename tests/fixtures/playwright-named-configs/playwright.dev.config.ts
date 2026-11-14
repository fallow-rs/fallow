import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './dev',
  testMatch: '*.pw.ts',
  globalSetup: './global-setup.ts',
  reporter: [['./dev-reporter.ts', { verbose: true }], ['list']],
});
