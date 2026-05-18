import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

// Canonical __dirname idiom for ESM: must not flag `./` as unresolved.
const __dirname = dirname(fileURLToPath(new URL('./', import.meta.url)))

export default defineConfig({
  root: __dirname,
})
