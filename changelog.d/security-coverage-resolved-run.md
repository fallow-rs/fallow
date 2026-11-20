### Fixed

- **`fallow security --runtime-coverage` uses the settings of its own run for the runtime coverage step.** The step loaded the config a second time and did not read the environment of the run, so a value such as `FALLOW_CACHE_DIR` did not reach it. The step now uses the config of the dead-code analysis of the run, with the same production mode as the files that it reuses.
  (Refs [#4875](https://github.com/fallow-rs/fallow/issues/4875))
