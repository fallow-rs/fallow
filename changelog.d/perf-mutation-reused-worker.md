### Changed

- **`fallow mutation` keeps one Vitest worker process for many mutant runs.** Vitest starts a new worker process for each run, and that start cost more CPU than most mutant runs. After the shared-worker check of the dry run passes, the Vitest adapter now keeps the worker process between mutant runs. Each test file of a reused run loads its modules again, so each mutant still gets fresh modules. A run whose tests add a global or change `process.env` ends the process, and a kill in such a run counts only when the test file also detects the mutant in a new worker. Dry runs, control runs and `--kill-matrix` runs still start a new worker.
  (Part of [#4341](https://github.com/fallow-rs/fallow/issues/4341))
