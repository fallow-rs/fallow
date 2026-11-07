### Fixed

- **`fallow mutation` reports tests that share a test ID.** Two tests of one file that ran, with the same full name (for example `test.each` with a title that has no placeholder), get the IDs `name` and `name (2)` in report order, so the evidence of these IDs can belong to the other test. The dry run now reports them in a `duplicate-test-ids` entry of `errors` (a warning in the human output), and the run goes on. A skipped or todo twin makes no duplicate. `--kill-matrix-for` with a test ID target that names a duplicate ID exits `2` with `FALLOW_MUTATION_TARGET_DUPLICATE_TEST_ID`. A test file target drops the duplicate IDs and keeps its other tests.
  (Closes [#4170](https://github.com/fallow-rs/fallow/issues/4170))
