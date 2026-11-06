### Changed

- **`fallow mutation --kill-matrix-for` gives each target its own evidence status.** Each entry of `kill_matrix.targets` now has a `status` (`complete` or `incomplete`) and its `reasons`. A mutant with incomplete evidence removes only the candidates of the targets that cover it. Before, one incomplete mutant removed the candidates of all targets. The run-wide reasons `partial-mutation-run`, `baseline-failure`, and `missing-test-file-metadata` still remove all candidates. The top-level `status` keeps its meaning, so `candidates` can be nonempty while `status` is `incomplete`. The human report shows the status of each target.
  (Closes [#4169](https://github.com/fallow-rs/fallow/issues/4169))
