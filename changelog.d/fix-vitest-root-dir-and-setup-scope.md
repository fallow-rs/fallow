### Fixed

- **Fallow now reads `test.root`, `test.dir` and the Vite `root` of a Vitest
  config.** The `test.include` globs are relative to `test.dir` below the
  Vitest root, and `setupFiles`, `globalSetup` and reporter paths are relative
  to the Vitest root. Before, Fallow read all of them relative to the config
  directory or the project root, so it reported test files and setup files as
  unused.
- **Vitest project files with another name now add their test files.** A
  project file such as `packages/a/vitest.unit.config.ts` in `test.projects`
  is the root of its project. Fallow now reads its `test.include` and its
  setup files relative to its own directory. A project without
  `test.include` runs the Vitest default include. Before, Fallow reported the
  test files of such a project as unused. An inline project in
  `test.projects` also gets the includes, the excludes, the `root` and the
  `dir` that it merges from its base config.
- **The default Vitest test patterns match `.mts`, `.cts`, `.mjs` and `.cjs`
  files.** Files such as `src/a.test.mts` and `tools/b.spec.cts` are test
  entry points without a config, as in Vitest.
