import { defineConfig } from 'vite-plus';

export default defineConfig({
  test: {
    include: ['e2e/**/*.e2e.ts'],
  },
});
