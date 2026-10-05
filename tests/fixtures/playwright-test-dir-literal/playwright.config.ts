import { defineConfig } from '@playwright/test';

export default defineConfig({ testDir: './ui[1]', testMatch: '*.pw.ts' });
