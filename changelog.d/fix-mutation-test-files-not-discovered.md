### Fixed

- **A mutation run in which the runner does not list a static test file is
  now `partial`.** The tests of such a file never run, but the run reported
  `completion.status: "complete"`. An empty survivor list then looked
  conclusive. The completion now has the new reason
  `test-files-not-discovered`, the same name as the `errors` entry that
  names the files. The exit code does not change.
  (Closes [#3596](https://github.com/fallow-rs/fallow/issues/3596))
