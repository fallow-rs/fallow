### Fixed

- Honor explicit error severity for `thin-wrapper` and `duplicate-prop-shape` in issue totals, dead-code and audit gates, and CI reports. Warnings remain advisory, including with `--fail-on-issues`. Per-file overrides and saved baselines use the component that owns each finding.
