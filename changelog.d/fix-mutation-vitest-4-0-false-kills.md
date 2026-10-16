### Fixed

- **`fallow mutation` no longer reports each mutant as killed on Vitest 4.0.**
  Vitest 4.0 reads the test-id filter of a test specification as a list of
  test lines. Each mutant run failed with an error, so the score was 100%
  also for mutants that no test detects. The supported Vitest range is now
  `>=4.1.0 <6.0.0`. On Vitest 4.0 the run exits with code 2 and
  `FALLOW_MUTATION_RUNNER_UNSUPPORTED`. The message names the installed
  version and the supported range. A runner version error of the companion
  now also gives this code instead of `FALLOW_MUTATION_RUNNER_FAILED`.
