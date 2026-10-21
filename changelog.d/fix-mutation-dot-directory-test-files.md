### Fixed

- **`fallow mutation` runs a mapped test file in a dot directory with the
  `node-test` and `bun-test` runners.** The glob discovery of these runners
  did not go into a directory such as `.github/`, so a test file there did
  not run, and its mutants were `no_coverage`. Fallow now sends the static
  test files to the runner. The runner adds each file that matches its
  default patterns. A file that the patterns do not match still makes the
  run `partial` with `test-files-not-discovered`.
  (Closes [#3678](https://github.com/fallow-rs/fallow/issues/3678))
