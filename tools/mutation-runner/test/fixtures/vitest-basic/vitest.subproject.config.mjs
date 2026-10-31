// A top-level root with a relative path. The Vitest CLI resolves it against
// the working directory, which is the project root.
export default {
  root: "./dashboard/",
  test: { include: ["src/math.test.ts"], maxWorkers: 1 },
};
