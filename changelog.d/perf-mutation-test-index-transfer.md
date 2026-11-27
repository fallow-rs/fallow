### Changed

- **`fallow mutation` with Vitest starts a restarted companion with no dry run.** A companion that starts in the mutant phase or the retry phase now takes the test index of the first dry run from fallow, and runs its jobs at once. Before, each of these companions did a dry run of the test files of its jobs first, so more workers could make a run slower and use more memory. A kill or a timeout by a test that the companion took counts only after a control run of that test file with no active mutant passes in the companion. A test that fails there is a baseline failure of that companion, as before. `node:test` and `bun test` keep the dry run.
  (Closes [#5419](https://github.com/fallow-rs/fallow/issues/5419))
