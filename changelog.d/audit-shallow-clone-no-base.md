### Fixed

- **`fallow audit` no longer passes silently in a shallow clone without a base.**
  A CI checkout with `fetch-depth: 1` has only the HEAD commit, so the
  auto-detected base is HEAD and the audit compares HEAD with itself. The
  audit now prints a warning on stderr, also under `--quiet`. The warning
  names the two fixes: `fetch-depth: 0` or `--base <ref>`. The JSON output
  reports `gate_outcomes["audit-verdict"].status` as `skipped`, so a CI
  integration shows that the run checked nothing. The exit code stays 0. An
  explicit `--base` or `FALLOW_AUDIT_BASE` keeps the old behavior.
  (Closes [#4839](https://github.com/fallow-rs/fallow/issues/4839))
