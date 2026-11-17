import { defineConfig } from '@playwright/test';

const external = Boolean(process.env.EXTERNAL_ORIGIN);

export default defineConfig({
  testDir: './e2e',
  reporter: external
    ? './reporters/conditional-reporter.ts'
    : [['./reporters/conditional-reporter.ts'], ['./reporters/second-reporter.ts']],
  globalSetup: external ? undefined : './conditional-setup.ts',
  globalTeardown: external ? './external-teardown.ts' : './local-teardown.ts',
});
