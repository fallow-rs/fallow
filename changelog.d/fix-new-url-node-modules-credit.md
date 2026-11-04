### Fixed

- **`fallow dead-code` no longer reports a package as unused when a `new URL()` path goes through its `node_modules` directory.** A relative string literal such as `new URL("./node_modules/icon-pack/svgs/", import.meta.url)` now credits `icon-pack`. A path without `./` (`node_modules/icon-pack/`) and a template literal with a static package name (`./node_modules/icon-pack/${name}.svg`) also credit `icon-pack`. A scoped path credits `@scope/pkg`. A pnpm store path credits the package after the last `node_modules` segment. This change bumps the extraction cache version and the graph cache version, so the first run after the upgrade parses all files again.
  (Closes [#4185](https://github.com/fallow-rs/fallow/issues/4185))
