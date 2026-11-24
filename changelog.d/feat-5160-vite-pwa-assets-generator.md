### Added

- **A `vite-pwa-assets-generator` plugin.** When `@vite-pwa/assets-generator` is a dependency, fallow no longer reports `pwa-assets.config.{ts,mts,cts,js,mjs,cjs,json}` as an unused file. The tool reads this file from the project root, so fallow credits it only at the project root and at the root of each workspace package. Under `includeEntryExports`, the `default` export of the config counts as used.
  (Closes [#5160](https://github.com/fallow-rs/fallow/issues/5160))
