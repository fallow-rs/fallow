// An inline project next to a project that a separate config file defines.
export default {
  test: {
    projects: [
      { test: { name: "unit", include: ["test/math.test.ts"] } },
      "projects/vitest.child.config.mjs",
    ],
  },
};
