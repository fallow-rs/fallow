### Fixed

- **`fallow mutation` no longer leaves the temp files of killed test runs on the disk.** When the runner killed a test run (for example after a timeout), the teardown of the test suite did not run, and its temp files stayed in the system temp directory. A large suite could fill the disk this way. Each companion now gets its own temp directory through `TMPDIR`, `TMP` and `TEMP`, and Fallow deletes it when the companion ends. The next run also deletes the directory of a Fallow process that ended on a signal or a crash. This applies to each runner (Vitest, `node:test` and `bun test`).
