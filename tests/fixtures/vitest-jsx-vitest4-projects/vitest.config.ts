import { defineConfig } from 'vitest/config'

export default defineConfig({
  oxc: { jsx: { importSource: './rt' } },
  test: {
    projects: [{ test: { name: 'p', include: ['src/a/**/*.test.tsx'] } }],
  },
})
