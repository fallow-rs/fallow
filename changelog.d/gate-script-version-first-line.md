### Fixed

- **The commit gate block message no longer includes the `verified:` line of the npm launcher.** The npm launcher prints a second `verified:` line for `fallow --version`. The gate script of `fallow setup-hooks` now reads only the first line of that output. The block message and the version floor check use only that version.
  (Closes [#4842](https://github.com/fallow-rs/fallow/issues/4842))
