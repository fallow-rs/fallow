// A top-level root with an absolute path from the location of the config file.
export default {
  root: new URL("./dashboard/", import.meta.url).pathname,
  test: { include: ["src/math.test.ts"], maxWorkers: 1 },
};
