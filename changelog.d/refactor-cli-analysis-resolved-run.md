### Changed

- **`fallow` checks the config once before the first analysis.** The bare run, `dead-code`, `dupes`, `health` and `audit` load, validate and resolve the config one time for each run. A config that fails validation stops the run with exit code 2 before it prints the `loaded config` line. The workspace discovery notice prints one time for each run. When the host cannot report its parallelism, fallow now starts one worker thread, not four.
