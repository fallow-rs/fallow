### Fixed

- **A bare `fallow` run with `--skip dupes` now writes an Impact record when `duplicates.enabled` is false.** The same is true for `--only dead-code,health`. With duplication off in the config, these flags do not narrow the run, and `fallow audit` already wrote a record for them. The bare run and the audit now use the same rule. When the project has duplication on, `--skip dupes` still writes no record in either command.
- **Impact no longer counts clone groups as resolved when `duplicates.enabled: false` turns the duplication analysis off.** A run without duplication keeps the clone groups that an earlier run stored. A later run with duplication credits a clone group only when the user removed it.
