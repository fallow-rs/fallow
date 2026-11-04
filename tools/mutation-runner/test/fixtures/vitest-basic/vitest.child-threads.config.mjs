export default {
  test: {
    include: ["child/*.test.ts"],
    pool: "threads",
    isolate: false,
  },
};
