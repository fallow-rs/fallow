### Added

- **`fallow mutation` reports the cost of the wide survivor retries.** The retry of a survivor runs each passing test of its test files, not only its covering tests, because a test can read a value that an earlier test of its file cached (#3670). The new `timings.retry_wide_worker_ms` field gives the time of these retries, summed over the workers. It is not a wall time, so it can be larger than `retry_ms`.
  (Refs [#5488](https://github.com/fallow-rs/fallow/issues/5488))

### Changed

- **`fallow mutation --time-budget` runs the cheapest retries first.** A few expensive survivor retries no longer leave the cheap retries without a run at the deadline. Each survivor keeps its own retry with each test of its files: the clean retry of one mutant does not prove that another mutant of the same function survives.
  (Refs [#5488](https://github.com/fallow-rs/fallow/issues/5488))
