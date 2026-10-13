export default {
  test: {
    include: ["test/**/*.test.ts"],
    setupFiles: ["./test/user-setup.ts"],
    // Longer than the timeout_ms of a run, so that only the companion can
    // stop a slow mutant early.
    testTimeout: 60_000,
    hookTimeout: 60_000,
  },
};
