### Fixed

- **A bare `fallow` run with `--skip dupes` now writes an Impact record when `duplicates.enabled` is false.** The same is true for `--only dead-code,health`. With duplication off in the config, these flags do not narrow the run, and `fallow audit` already wrote a record for them. The bare run and the audit now use the same rule. When the project has duplication on, `--skip dupes` still writes no record in either command.
