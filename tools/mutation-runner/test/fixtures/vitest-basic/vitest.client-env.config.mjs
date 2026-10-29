// Two inline projects run one file: one in the node environment and one in a
// client environment, as with `happy-dom`.
export default {
  test: {
    projects: [
      { test: { name: "node", include: ["client/client.test.ts"] } },
      {
        test: {
          name: "client",
          include: ["client/client.test.ts"],
          environment: "./client/client-environment.mjs",
        },
      },
    ],
  },
};
