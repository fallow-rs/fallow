### Fixed

- **`fallow mutation` now runs Vitest projects in a `happy-dom` or `jsdom` environment when the cache is outside the project.** A client test environment loads the setup file of the mutation runtime through Vite. Vite refused that file when `FALLOW_CACHE_DIR` was outside the project, so the project failed its baseline with `Cannot find module '/@fs/.../vitest-setup.mjs'`. The Vitest adapter now adds the companion directory to `server.fs.allow` of the root server and of each inline project.
  (Closes [#3793](https://github.com/fallow-rs/fallow/issues/3793))
