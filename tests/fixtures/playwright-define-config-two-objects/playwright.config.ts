import { defineConfig } from '@playwright/test';

export default defineConfig({ testDir: './a' }, { testDir: './e2e', testMatch: '*.pw.ts' });
