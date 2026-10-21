import { defineConfig } from 'vite';

export default defineConfig({
  pack: {
    entry: ['./src/pack-only.ts'],
  },
});
