### Fixed

- **`fallow mutation` keeps a survivor in the score when its retry times
  out.** A survivor ran all its tests with the mutant active. Host load can
  make the retry time out, and that timeout is no proof that the tests
  detect the mutant. A survivor with a timed-out retry is now `survived`,
  not `flaky`. A survivor with a killed retry is still `flaky`.
- **`fallow mutation` exits with code 2 when baseline failures stop each
  mutant run.** A run in which no mutant got a test result (`killed`,
  `timeout`, `survived` or `flaky`) because of baseline failures measured
  nothing, but it exited with code 0. It now exits with code 2 and
  `FALLOW_MUTATION_BASELINE_FAILED`. The error names the first failure and
  the first line of its message. With `--format json`, stdout gets the
  error envelope. The run still saves the report to `last-report.json` and
  to the `-o` file, and `help` gives the paths. A run with a `pending`
  mutant, or with only `no_coverage` mutants and no baseline failure, still
  exits with code 0.
- **The human `fallow mutation` report shows the message of the first
  baseline failure.** A `Message` line under the baseline failure line shows
  the first line of the message, for example `Invalid environment
  variables`. Before, the message was only in the `--format json` output
  when the failure had a test id.
