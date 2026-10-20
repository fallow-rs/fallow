### Fixed

- **A Vitest `test.include` no longer makes bench files unused.** The bench
  files are entry points again when a config sets `test.include`. Fallow now
  reads `test.benchmark.include`, and the default bench pattern is the Vitest
  default `**/*.{bench,benchmark}.?(c|m)[jt]s?(x)`. Before, the default pattern
  did not match `.benchmark.` files or the `.mts` and `.cts` extensions.
- **A Vitest config below the project root no longer replaces the test entry
  points of the whole project.** The `test.include` globs of such a config,
  for example a fixture config, now apply below the directory of that config.
  Only a config in the project root or the workspace root replaces the default
  test patterns. A config with another name than `vitest.config.<ext>` or
  `vite.config.<ext>` loads through `--config`, so its globs stay relative to the
  package root. A replacing config also keeps the patterns of the configs
  that Fallow read before it.
- **A Vitest reporter path is now a used file, not a dependency.** A
  `reporters` entry such as `'./scripts/ci-reporter.ts'` marks that file as
  used. Before, Fallow read the path as a package name and reported the file
  as unused. Fallow also reads a reporter in a spread array, such as
  `...(process.env.CI ? ['./scripts/ci-reporter.ts'] : [])`.
