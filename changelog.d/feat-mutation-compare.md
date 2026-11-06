### Added

- **`fallow mutation --compare <file>` lists the mutants that a test change no longer detects.** Save a complete report, change the tests, then run `fallow mutation --compare <file>`. The new `compare` object lists each mutant that was `killed` or `timeout` in the saved report and is `survived` or `no_coverage` now as `regressed`, with the earlier `previous_killing_tests`. The human output names the test that killed the mutant before. A `flaky` mutant in either run is `inconclusive`, and a mutant id in one run only is `added` or `removed`. A saved report or a run that is not complete exits 2 with `FALLOW_MUTATION_COMPARE_INCOMPLETE`.
  (Closes [#4168](https://github.com/fallow-rs/fallow/issues/4168))
