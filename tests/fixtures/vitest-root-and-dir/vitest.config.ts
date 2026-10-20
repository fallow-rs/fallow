export default {
  test: {
    root: 'app',
    dir: 'tests',
    include: ['**/*.check.ts'],
    setupFiles: ['./setup.ts'],
  },
};
