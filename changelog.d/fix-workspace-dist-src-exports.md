### Fixed

- **A workspace import now keeps its credit when the package `exports` point into `dist/src/` and the build output exists.** A package with tsconfig `rootDir: "./"` and `outDir: "./dist"` emits `src/index.ts` as `dist/src/index.js`. Fallow now uses the `rootDir` and `outDir` of the package tsconfig to map that output back to `src/index.ts`. Before, the imported exports were reported as unused after a build. Thanks to @Smrtnyk for the report (#3817).
- **A warm cache now gives the same result as `--no-cache` after the build output of a workspace package appears or goes away.** The graph cache key now includes whether each static entry path in the project and workspace `package.json` files exists on disk. Thanks to @Smrtnyk for the report (#3818).
