### Fixed

- **The graph cache now misses when Yarn Plug'n'Play files change.** Before this fix, a run after a switch to the Yarn PnP linker, or after a change to `.pnp.cjs` with the same `yarn.lock`, could use the import resolutions of the run before. The resolver reads `.pnp.cjs` at the project root and above it, so the graph cache key now includes the content of `.pnp.cjs` and `.pnp.data.json` at the root, at each workspace root, and in each directory above the root.
