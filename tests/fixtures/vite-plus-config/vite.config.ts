import { defineConfig } from 'vite-plus';

export default defineConfig({
  pack: [
    { entry: { index: './src/index.ts' }, format: ['esm'] },
    { entry: { 'gitlab/index': './src/gitlab/index.ts' } },
    { entry: ['./src/azure/index.ts'] },
  ],
  lint: {
    jsPlugins: ['@e18e/eslint-plugin', { name: 'local', specifier: './lint/local-plugin.js' }],
    overrides: [{ files: ['**/*.test.ts'], jsPlugins: ['eslint-plugin-override-only'] }],
  },
  fmt: {
    ignorePatterns: ['dist/**'],
  },
  staged: {
    '*.ts': 'staged-tool --fix',
  },
  run: {
    tasks: {
      generate: {
        command: 'task-tool generate && node scripts/task-only.ts',
      },
      short: 'node scripts/task-short.ts',
    },
  },
});
