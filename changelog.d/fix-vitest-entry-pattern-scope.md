### Fixed

- **A Vitest `test.include` no longer makes bench files unused.** The bench
  files are entry points again when a config sets `test.include`. Fallow now
  reads `test.benchmark.include`, and the default bench pattern is the Vitest
  default `**/*.{bench,benchmark}.?(c|m)[jt]s?(x)`. Before, the default pattern
  did not match `.benchmark.` files or the `.mts` and `.cts` extensions.
- **A Vitest config below the project root no longer replaces the test entry
  points of the whole project.** Vitest finds a `vitest.config` or
  `vite.config` file (`.ts`, `.js`, `.mts`, `.mjs`, `.cts` or `.cjs`) in the
  directory that it runs in. The `test.include` globs of such a file below
  the root, for example a fixture config, now apply below its own directory.
  Only a config in the project root or the workspace root replaces the default
  test patterns. The globs of a config that a package script loads with
  `--config` stay relative to the package root. The configs of one project
  now add up, so no config removes the patterns of another config. A
  `vite.config` file next to a `vitest.config` file gives no includes, unless
  the `vitest.config` file names it. After the upgrade, `fallow dead-code` can
  report files that only a fixture config marked as used before. You can
  remove an `entry` workaround for test files that Vitest finds.
- **A Vitest reporter path is now a used file, not a dependency.** A
  `reporters` entry such as `'./scripts/ci-reporter.ts'` marks that file as
  used. Before, Fallow read the path as a package name and reported the file
  as unused. Fallow also reads a reporter in a spread array, such as
  `...(process.env.CI ? ['./scripts/ci-reporter.ts'] : [])`. The config reader
  of the other plugins now also reads spread arrays and both branches of a
  condition.
