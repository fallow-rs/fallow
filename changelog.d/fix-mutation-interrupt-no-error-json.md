### Fixed

- **A Ctrl+C during the dry run of `fallow mutation` no longer prints a runner error.** The signal kills the companion, and the run reported that exit as `FALLOW_MUTATION_RUNNER_FAILED` with `"exit_code": 2`, while the process exited with `130`. Fallow now prints no error while a signal ends the process, so the exit code and the output agree. A run that a signal stops during the mutant runs still prints the report of the finished mutants. Closes #4302.
