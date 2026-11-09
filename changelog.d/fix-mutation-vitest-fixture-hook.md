### Fixed

- **`fallow mutation` no longer fails each Vitest test that uses a `test.extend` fixture.** The `afterEach` hook of the Vitest setup file now takes an object pattern as its first argument. Vitest reads that argument for the fixtures of a test, and before this fix each fixture test failed with `FixtureParseError` in the dry run. The help of a run that a baseline failure blocks now points at `baseline_failures[].message` in the JSON report, which holds the error of each failed test, and no longer guesses environment variables as the cause.
  (Closes [#4423](https://github.com/fallow-rs/fallow/issues/4423))
