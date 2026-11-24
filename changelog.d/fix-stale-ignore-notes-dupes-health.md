### Fixed

- **`fallow dupes` and `fallow health` now report a `duplicates.ignore` or `health.ignore` glob that matches no file.** Only a dead-code run gave this note, so a CI job that ran only one of these commands never showed the stale entry. A `fallow dupes` run adds `duplicates-ignore-unmatched` to `workspace_diagnostics`, and a `fallow health` run adds `health-ignore-unmatched`. The human output prints one note per setting, also in a `fallow --only dupes` or `fallow --only health` run. The combined `fallow` run states each entry once. A `--production` run reports none of these.
  (Closes [#5202](https://github.com/fallow-rs/fallow/issues/5202))
