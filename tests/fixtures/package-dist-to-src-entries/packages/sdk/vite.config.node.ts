import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    outDir: 'dist',
    lib: {
      entry: {
        cli: 'src/cli/cli.ts',
      },
    },
  },
});
