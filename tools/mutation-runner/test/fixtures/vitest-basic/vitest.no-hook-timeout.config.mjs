export default {
  test: {
    include: ["guards/slow-hook.test.ts"],
    // Vitest reads a hook timeout of 0 as no limit.
    hookTimeout: 0,
  },
};
