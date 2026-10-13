// An inline project with a browser block that does not enable browser mode.
// Vitest 5 does not give the plugins of `createVitest` to such a project.
export default {
  test: {
    projects: [
      {
        test: {
          name: "unit",
          include: ["test/math.test.ts"],
          setupFiles: ["./test/user-setup.ts"],
          browser: { enabled: false, instances: [{ browser: "chromium" }] },
        },
      },
    ],
  },
};
