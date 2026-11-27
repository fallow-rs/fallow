### Added

- **`fallow mutation` reports the worker mode of the mutant runs.** The new `runner.worker_modes` object of the JSON report gives the number of mutant runs and their time in each mode of the Vitest companion: `isolated` (a new worker for each test file), `shared` (one new worker for all test files of the run) and `reused` (the kept worker process of the companion). A slow run with a high `isolated` value now explains itself. (Refs [#5423](https://github.com/fallow-rs/fallow/issues/5423))
