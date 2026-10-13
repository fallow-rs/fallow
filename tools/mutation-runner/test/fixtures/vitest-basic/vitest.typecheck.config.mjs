// The type-check pass is on, but the companion never runs it.
export default {
  test: {
    include: ["test/math.test.ts"],
    setupFiles: ["./test/user-setup.ts"],
    typecheck: { enabled: true },
  },
};
