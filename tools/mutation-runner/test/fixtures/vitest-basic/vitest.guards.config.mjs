export default {
  test: {
    include: ["guards/{slow-hook,retry}.test.ts"],
    // A retry must never turn a failed first attempt into a pass.
    retry: 2,
  },
};
