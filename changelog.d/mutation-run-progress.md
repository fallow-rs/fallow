### Added

- **A human `fallow mutation` run now shows phase progress on stderr.** The
  lines name the runner setup, the baseline run, the mutant runs with
  done and total counts, the retries, and the final worktree check and
  report. A long phase prints a line at a fixed interval, also in captured
  non-TTY output, so a slow run does not look stalled. `--quiet` and
  `--format json` print no progress lines, and stdout does not change. The
  programmatic API has `run_mutation_with_progress` with the
  `MutationProgress` events.
  (Closes [#3599](https://github.com/fallow-rs/fallow/issues/3599))
