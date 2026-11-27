### Changed

- **The `fallow mutation` self-check of a wide first job runs on more than one companion.** Before the first mutant runs, a control run with no active mutant checks the run path with the tests of the first job. It ran on one companion, so a first job with many test files took as long as an unsplit mutant run, and the other companions stayed idle. When the tests of the first job are in the dry-run shares of more than one companion, each of these companions now checks its own tests at the same time. The parts give one result with the rules of one control run.
  (Closes [#5444](https://github.com/fallow-rs/fallow/issues/5444))
