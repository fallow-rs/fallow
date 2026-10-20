export default {
  test: {
    include: ["require/*.test.ts"],
    pool: "threads",
    isolate: false,
  },
};
