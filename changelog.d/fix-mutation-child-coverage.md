### Fixed

- **`fallow mutation` now selects a test that asserts only on the output of a Node child process.** With the Vitest and `node:test` runners, a Node process that a test starts (spawn, exec, execFile or fork) now gives its coverage hits to that test in the dry run. Before, such a test had no hits, so the run did not select it for a mutant. When no other test file detected the mutant, the report showed a false survivor. A child process that a signal ends gives no hits.
  (Closes [#4171](https://github.com/fallow-rs/fallow/issues/4171))
