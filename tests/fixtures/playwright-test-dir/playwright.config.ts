import { defineConfig } from '@playwright/test';

export default defineConfig({ testDir: './ui', testMatch: '*.pw.ts' });
