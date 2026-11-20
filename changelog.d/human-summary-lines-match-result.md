### Fixed

- **The summary lines of the human output now agree with the result of the run.**
  The `Failed:` line of a bare `fallow` run names only the analyses whose gate
  fails the run. Clone groups below the duplication threshold and health
  findings that a baseline accepts no longer appear on it. The health status
  line counts only the functions that the baseline does not accept, and names
  the accepted functions, for example `✓ 0 above threshold (3 in the baseline)`.
  `fallow health --css` adds the count of styling findings to the status line.
  `fallow audit` ends with `No introduced issues` when the gate leaves out
  inherited findings. The health score tip names `duplicates.ignore`, the key
  that removes files from the duplication analysis.
  (Closes [#4837](https://github.com/fallow-rs/fallow/issues/4837))
