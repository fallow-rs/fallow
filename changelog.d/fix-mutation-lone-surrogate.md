### Fixed

- **`fallow mutation` accepts a test name with a lone UTF-16 surrogate.** Before, a Vitest test name such as `"a\uD800"` stopped the dry run with `FALLOW_MUTATION_RUNNER_FAILED` and the message `a response line is not JSON`. Now the companion writes each lone surrogate as U+FFFD, in the test id and in each other string of a response. A mutant run selects the test with the same id, so the test still runs. (Closes [#4244](https://github.com/fallow-rs/fallow/issues/4244))
