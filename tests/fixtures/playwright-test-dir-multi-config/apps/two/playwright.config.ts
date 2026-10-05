import { defineConfig } from '@playwright/test';

export default defineConfig({ testDir: './checks', testMatch: '*.pw.ts' });
