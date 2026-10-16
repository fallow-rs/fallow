### Fixed

- **`fallow mutation` now needs Bun 1.4.0 or later for `bun test`.** On Bun
  1.3.x the adapter got no test results for a run with `--bail`, and it
  reported each thrown error as an assertion failure. The supported Bun range
  is now `>=1.4.0 <2.0.0`. On Bun 1.3.x the run exits with code 2 and
  `FALLOW_MUTATION_RUNNER_UNSUPPORTED`. The message names the installed
  version and the supported range, and the help gives the install step.
