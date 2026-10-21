### Added

- **A dead-code baseline now records whether `node_modules` existed.** A
  `--save-baseline` run writes `analysis_identity.node_modules` as `present`
  or `missing`. A later run in the other install state adds a
  `baseline-install-state-mismatch` entry to `workspace_diagnostics` and
  prints a warning, also under `--quiet`. Findings that need installed
  packages can differ between the two states, so the run can report baseline
  findings as new. Save the baseline again once to get this check. A baseline
  without the key loads as before.

### Fixed

- **`ignoreFindings` now covers the parse errors of the files it hides.** A
  syntax-error test fixture that `ignoreFindings` covers no longer reports
  `source-parse-degraded`, no longer prints a parse warning, and no longer
  fails the `parse-error` gate. Its partial parse also no longer marks other
  findings with the `incomplete-import-graph` caveat, and the pattern no
  longer reads as a pattern that matched no finding.
- **`--quiet` now hides the analysis warnings on stderr.** A run with
  `--quiet`, `--ci` or `FALLOW_QUIET` no longer prints warnings that repeat a
  `workspace_diagnostics` entry, such as parse degradations, the tsconfig
  chain warning, re-export cycles and skipped `package.json` entry points.
  The JSON output does not change. Set `RUST_LOG=warn` to show them again.
  Config warnings, such as an unknown rule name, still print.
