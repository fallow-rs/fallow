import { defineConfig } from 'vitest/config'

export default defineConfig({
  oxc: { jsx: { importSource: './rt-root' } },
  test: {
    projects: [
      './nested/vitest.config.ts',
      './e2e/vitest.e2e.config.ts',
      'runtime/*/vitest.e2e.config.ts',
      { test: { name: 'inherit', include: ['inherit/**/*.test.tsx'] } },
      {
        extends: './base.config.ts',
        test: { name: 'based', include: ['based/**/*.test.tsx'] },
      },
      {
        extends: false,
        oxc: { jsx: { importSource: './rt-skip' } },
        test: { name: 'skipped', include: ['skipped/**/*.test.tsx'], exclude: ['skipped'] },
      },
    ],
  },
})
