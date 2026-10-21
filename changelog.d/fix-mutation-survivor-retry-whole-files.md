### Fixed

- **`fallow mutation` no longer reports a false survivor when a test reads a
  cached value.** The per-test hits of the dry run name only the tests that
  execute the mutated code. A test that reads a value that an earlier test of
  its file cached gets no hit, so no run of the mutant included it. The retry
  of a survivor now runs each test of the files of its covering tests. A kill
  by a test that the first run did not execute gives `killed`, not `flaky`,
  when the same tests pass in a control run with no active mutant. The first
  run of each mutant stays narrow. `covering_tests` still lists the tests
  that reached the mutant in the dry run, so `killed_by` can name a test
  that is not in that list. With `--time-budget`, the time that the first
  pass keeps free for a survivor retry grows with the tests of its files.
  (Closes [#3670](https://github.com/fallow-rs/fallow/issues/3670))
