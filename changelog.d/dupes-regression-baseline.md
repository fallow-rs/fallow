### Fixed

- **`fallow dupes` rejects the regression flags instead of a silent run.**
  Before, `fallow dupes --save-regression-baseline` exited 0 and wrote no
  file, and `--fail-on-regression`, `--regression-baseline` and `--tolerance`
  compared nothing. Only bare `fallow`, `fallow dead-code`, `fallow
  architecture`, `fallow flags --retirement` and `fallow list --entry-weight`
  save or compare a regression baseline. The other commands, for example
  `fallow dupes`, `fallow health`, `fallow audit` and a bare run without the
  dead-code analysis, now exit 2 and name the commands that support the
  flags. For `fallow dupes` and `fallow health`, the message also names
  `--baseline`, which gates on new findings.
  (Closes [#4931](https://github.com/fallow-rs/fallow/issues/4931))
