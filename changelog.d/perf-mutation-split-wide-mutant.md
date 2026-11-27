### Changed

- **`fallow mutation` runs a widely covered mutant on more than one worker.** Before, one mutant ran on one worker, also when its covering tests were in many test files, and the other workers stayed idle. With Vitest, a mutant that would outlast the work of the other workers now runs in parts of whole test files on the idle workers. The survivor retry uses the same rule. The verdicts follow the rules of one run: a kill in any part decides `killed`, `killed_by` is the killing test with the smallest test id, and the mutant is `survived` only when each part passes. `duration_ms` is the wall time of the mutant. The result cache key does not change.
  (Closes [#5422](https://github.com/fallow-rs/fallow/issues/5422))
