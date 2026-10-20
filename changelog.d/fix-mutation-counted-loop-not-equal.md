### Fixed

- **`fallow mutation` no longer makes an equivalent `!=` mutant for the test
  of a counted `for` loop.** For a `let` counter that starts at an integer
  literal, moves one step to an integer literal bound, and has no other write,
  `i < N` and `i != N` select the same iterations. No test can kill that
  mutant. The `<=` and constant mutants of the test do not change.
  (Closes [#3649](https://github.com/fallow-rs/fallow/issues/3649))
