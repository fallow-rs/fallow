### Fixed

- **`fallow mutation` runs one Vitest worker in each companion when `VITEST_MAX_WORKERS` is set.** Vitest applies `VITEST_MAX_WORKERS` after the config options. An inherited value thus replaced the `maxWorkers: 1` of the companion, and each companion started that many workers. With the default `--concurrency`, this multiplied the CPU and memory use. The Vitest companion now removes the variable before it starts Vitest, so `--concurrency` is the only control of test parallelism.
  (Closes [#4613](https://github.com/fallow-rs/fallow/issues/4613))
