export default {
  test: {
    include: ["test/**/*.test.ts"],
    setupFiles: ["./test/user-setup.ts"],
    isolate: false,
  },
};
