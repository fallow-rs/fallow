### Added

- **`fallow mutation --format json` keeps the history of a retried mutant.**
  A retried mutant result has a new `attempts` array with the first run and
  the retry. Each entry has the status and the duration of that run, and the
  killing test, the failure kind (`assertion`, `error` or `timeout`) or the
  runtime-error reason when the run has one. A `timeout -> survived` result
  and a `survived -> killed` result are both `flaky`, and the saved report
  now shows which history each one has. The `status`, `duration_ms` and
  `retried` fields do not change.
  (Closes [#3656](https://github.com/fallow-rs/fallow/issues/3656))
