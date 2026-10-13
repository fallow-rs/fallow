// Vitest resolves this project from the directory of this file.
export default {
  test: {
    name: "child",
    include: ["shared.test.ts"],
    provide: { fixtureProject: "a" },
  },
};
