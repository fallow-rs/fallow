### Fixed

- **The `fallow audit` scope line now starts the compared range at the base commit.** The line showed the `HEAD` commit as the start of the range, for example `vs main (<head-sha>..HEAD)`. In git syntax, that range is empty. The line now shows the merge-base of the base ref and `HEAD`, which is the commit that the audit diff starts from. The JSON output does not change.
  (Closes [#4838](https://github.com/fallow-rs/fallow/issues/4838))
