import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    outDir: 'dist/define',
    lib: {
      entry: 'src/sdk/define/index.ts',
      fileName: (format) => `index.${format === 'es' ? 'mjs' : 'cjs'}`,
    },
  },
});
