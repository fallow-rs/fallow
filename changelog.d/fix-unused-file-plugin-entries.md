### Fixed

- **`unused-file` now credits four more framework entry points.**
  - React Email: an `email dev`, `email build`, `email start` or `email export` command in a package script or a mise task makes the `.js`, `.jsx` and `.tsx` templates of its `--dir` folder (default `emails/`) entry points.
  - Docusaurus: a nested theme component with a `PascalCase` name, such as `src/theme/NavbarItem/ComponentTypes.js`, is an entry point. A lowercase helper file in a swizzled folder stays an ordinary module.
  - Config values: when a config property names a top-level array binding, fallow reads the array values and the static values that top-level `push`, `unshift` and `splice` calls add, also in an `if`, a block or a `try` statement. A Vitest `globalSetup` file that a condition adds is now a setup file.
  - mise: a task that runs build output, such as `node ./dist/bin/sync.js`, credits the source file through the `tsconfig` `outDir` and `rootDir` of the task directory.
  (Closes [#5338](https://github.com/fallow-rs/fallow/issues/5338), [#5339](https://github.com/fallow-rs/fallow/issues/5339), [#5340](https://github.com/fallow-rs/fallow/issues/5340), [#5341](https://github.com/fallow-rs/fallow/issues/5341))
