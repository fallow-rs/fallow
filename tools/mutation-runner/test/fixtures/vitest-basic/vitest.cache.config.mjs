// Turns on the persistent transform cache on both majors. The companion must
// turn it off, so that a cached transform never replaces the run-dir file.
export default {
  test: {
    include: ["test/**/*.test.ts"],
    setupFiles: ["./test/user-setup.ts"],
    fsModuleCache: true,
    experimental: { fsModuleCache: true },
  },
};
