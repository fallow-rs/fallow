// Two projects include the same test file with a different provided value.
export default {
  test: {
    projects: [
      {
        test: { name: "a", include: ["projects/shared.test.ts"], provide: { fixtureProject: "a" } },
      },
      {
        test: { name: "b", include: ["projects/shared.test.ts"], provide: { fixtureProject: "b" } },
      },
    ],
  },
};
