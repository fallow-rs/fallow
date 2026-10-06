import { defineConfig } from '@playwright/test';
import base from './base.config';

export default defineConfig(base, { testDir: './e2e' });
