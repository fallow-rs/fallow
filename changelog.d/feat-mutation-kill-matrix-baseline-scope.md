### Changed

- **`fallow mutation --kill-matrix-for` gives `baseline-failure` only to the targets that the failure can affect.** Before, one test that failed with no active mutant made each target `incomplete`. A broken test anywhere in the suite then blocked every target. Now a target gets `baseline-failure` only in these cases:
  - The failing test is a test of the target, or it is in a test file of the target.
  - The failing test failed a control run or the dry run of a restarted companion. It also covers, kills, or ran without a mutant that a target test covers or kills.

  A target can now be `complete` in a run with baseline failures, so its tests can be `candidates`. When baseline failures are the only cause of a partial run, only the affected targets get `partial-mutation-run`. Each other cause of a partial run still applies to each target. The top-level `status`, `reasons`, and `completion` do not change. Shared state across test files is outside the rule.

  A failing test no longer makes the kill of a target test shared. A passing test in the test file of a baseline failure also does not make a kill shared. Such a kill stays in `unique_kills`. The human report names the baseline failures of each target under its line.
  (Closes [#4469](https://github.com/fallow-rs/fallow/issues/4469))
