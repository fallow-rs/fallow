### Fixed

- **`fallow mutation` with `node:test` now serves the mutant to a Node child
  process of a test.** A test that ran a CLI with `execFileSync` of
  `process.execPath` loaded the original source in that child. So an exact
  CLI assertion could not kill the mutant, and the report showed a false
  survivor. The test process now adds the overlay preload to `NODE_OPTIONS`.
  Each Node child process (`spawn`, `exec`, `execFile` or `fork`) then loads
  the instrumented files with the active mutant. The hits of a child process
  do not count as coverage. A child that is not Node, or a child with an
  environment without `NODE_OPTIONS`, still loads the original files.
  (Closes [#3680](https://github.com/fallow-rs/fallow/issues/3680))
