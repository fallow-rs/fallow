import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: [
      {
        oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
        extends: true,
        test: { include: ['src/**/*.test.tsx'], name: 'main' },
      },
      {
        oxc: { jsx: { runtime: 'automatic', importSource: './missing/jsx' } },
        extends: true,
        test: { include: ['other/**/*.test.tsx'], name: 'other' },
      },
    ],
  },
})
