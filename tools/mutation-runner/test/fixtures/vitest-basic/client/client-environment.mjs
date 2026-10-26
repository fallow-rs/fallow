// A test environment that loads its modules through the Vite client
// environment, as `happy-dom` and `jsdom` do. Vite serves such a module only
// from a directory in `server.fs.allow`.
export default {
  name: "fallow-client",
  viteEnvironment: "client",
  setup: () => ({ teardown: () => {} }),
};
