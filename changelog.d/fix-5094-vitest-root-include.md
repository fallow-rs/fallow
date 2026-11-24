### Fixed

- **A root Vitest `test.include` no longer makes the test files of a workspace unused.** The include of the root config no longer replaces the test patterns of a workspace. A workspace keeps its own include or the default test patterns. A Vitest config that is only `export { default } from "<module>"` now gives the includes of that module, when the module is a sibling file or a package in the project.
  (Closes [#5094](https://github.com/fallow-rs/fallow/issues/5094))
