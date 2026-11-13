### Fixed

- **A `vite.config.*` file in a workspace package is now an entry point when only the root manifest lists `vite` or `vite-plus`.** Before, fallow credited the packages that such a config imports, but it did not follow the relative imports of the config. A shared helper that only the package configs import, such as a root `vite.package.ts`, was then reported as an unused file. You can now remove `packages/*/vite.config.ts` and the helper from `entry`.
