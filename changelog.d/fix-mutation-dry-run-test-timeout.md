### Fixed

- **`fallow mutation` no longer reports a slow test as a baseline failure when the test is close to its timeout.** The dry run executes the instrumented code, which is slower than the original code. The dry run now gives each test three times the test timeout of the project. Vitest gets three times the `testTimeout` and `hookTimeout` of each project, and `bun test` gets three times the Bun default. Mutant runs keep their timeout from the dry-run durations. (Closes [#4409](https://github.com/fallow-rs/fallow/issues/4409))
