export default {
  test: {
    include: ["guards/fd1.test.ts"],
    // Workers of the threads pool share file descriptor 1 with the companion.
    pool: "threads",
  },
};
