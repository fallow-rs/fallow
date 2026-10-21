### Changed

- **`fallow audit` no longer fails on a complexity finding in a test file.**
  A long `it(...)` or `describe(...)` callback often exceeds the cognitive
  threshold. Such a finding now has `effective_severity: warn` and the new
  field `gate_exemption: "test-file"`. It stays in the report with its
  attribution, and the verdict is at least `warn`, with exit code 0. This
  applies to `--gate new-only` and `--gate all`. SARIF reports the finding at
  level `warning` with `gate_exemption` in the property bag. The human,
  markdown and compact views list these findings under "Test files, not
  gated".

  The shared test-path rule decides what a test file is: `*.test.*`,
  `*.spec.*`, `*.e2e.*`, files below `__tests__/`, `test/`, `tests/` or `e2e/`,
  mocks and fixtures. `fallow health` does not change: it still reports these
  findings at the severity of their `complexity-*` rule.
