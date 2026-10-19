### Added

- **`fallow mutation --runner-config <path>` selects the Vitest config of
  a run.** A project that keeps its integration, UI or browser tests in a
  separate Vitest config can now run mutants against those tests. The
  `mutation.runnerConfig` config key sets the same file, and the flag
  wins. The file must exist inside the project root, and only the Vitest
  runner accepts it. A static test file that the selected config does not
  include stays visible as a `test-files-not-discovered` error. `--config`
  still selects the Fallow config.
  (Closes [#3598](https://github.com/fallow-rs/fallow/issues/3598))
