### Changed

- **`fallow mutation` gives a loop timeout of short tests no retry.** The loop budget of a mutant run now counts the CPU time of the test thread, not the wall time. A test that waits uses no budget, and load from other processes on the host does not count. A loop timeout of covering tests that take 200 ms or less in the dry run is final: the budget is then at least four times the work of the tests. Before, each such mutant ran a second time with twice the budget.
  (Part of [#4341](https://github.com/fallow-rs/fallow/issues/4341))
