// One inline project that extends the root config. Vitest copies the root
// `bail` value into the project config when it creates the project.
export default {
  test: {
    include: [],
    projects: [{ extends: true, test: { name: "inline", include: ["bail/three.test.ts"] } }],
  },
};
