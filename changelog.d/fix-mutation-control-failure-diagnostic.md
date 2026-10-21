### Fixed

- **`fallow mutation` now keeps the error of a test that fails in a control
  run.** A control run runs the covering tests with no active mutant. When a
  test fails there, the `baseline_failures` message now adds the error of the
  test after the reason: the assertion on the first line, then the stack
  frames of the project. The `FALLOW_MUTATION_BASELINE_FAILED` error and
  the `Error:` line of the human footer also show the assertion. Before, the
  message gave the reason only. Paths are relative to the project, and the
  error is about 4 KiB at most. The verdict does not change: the run still
  stops with exit code 2.
  (Closes [#3690](https://github.com/fallow-rs/fallow/issues/3690))
