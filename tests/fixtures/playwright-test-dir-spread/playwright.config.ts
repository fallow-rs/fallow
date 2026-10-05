import { defineConfig } from '@playwright/test';

const shared = { testDir: './tests' };
export default defineConfig({ testDir: './ui', testMatch: '*.pw.ts', ...shared });
