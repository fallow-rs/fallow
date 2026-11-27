### Added

- `fallow mutation` reports how the Vitest companion used worker processes. The new `runner.worker_modes` object of the JSON report gives the number of mutant runs and their time in each mode: `isolated` (a new worker for each test file), `shared` (one new worker for all test files of the run) and `reused` (the kept worker process of the companion). A slow run with a high `isolated` count now explains itself. ([#5423](https://github.com/fallow-rs/fallow/issues/5423))
