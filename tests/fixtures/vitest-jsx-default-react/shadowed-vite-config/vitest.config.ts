import { defineConfig } from 'vitest/config'

export default defineConfig({
  oxc: { jsx: { importSource: 'preact' } },
  test: {},
})
