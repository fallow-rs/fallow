### Fixed

- **`fallow health --baseline-mode identity` no longer lets one saved slot hide two findings.** A slot of an `<anonymous>` or `<arrow>` function that hides a finding of a lower severity in that function stays in use. A new named function in the same file does not get it.
- **`fallow baselines prune` keeps the health slot of an unnamed function that covers a named function of the same file.** Before, the prune removed that slot, and the next identity run reported the function.
- **The `baseline_staleness` counts of `fallow health --baseline` agree with the findings that the baseline hides.** A saved slot of a lower severity than the current finding no longer counts as matched. `--fail-on-stale-baseline` can therefore fail on a baseline that passed before.
