export default {
  test: {
    include: ["child-split/*.test.ts"],
    pool: "threads",
    isolate: false,
  },
};
