### Fixed

- **The output schema and the issue registry describe the JSON that the
  commands print.** `docs/output-schema.json` changes:
  - Each root envelope definition, such as `CheckOutput`, `DupesOutput`,
    `HealthOutput`, `AuditOutput` and `FeatureFlagsOutput`, has an optional
    `kind` property with the value of its command. An envelope nested in
    another envelope has no `kind`.
  - The `feature-flags` branch text names `feature_flags` and `total_flags`.
  - `HealthOutput.health_score` and `file_scores` say that a plain
    `fallow health` run prints them.
  - `Meta` and the `_meta` field texts say that a `--format json` run
    always sets `telemetry.analysis_run_id`.
  - `BoundaryViolation.import_specifier` says that the field holds the
    resolved project-root-relative path.

  `fallow schema` changes:
  - `unused-catalog-entry` is suppressible, with the YAML comment
    `# fallow-ignore-next-line unused-catalog-entry`.
  - The notes of the import-cycle, boundary and policy rows say that
    `fallow architecture` also reports them.

  The `fallow explain constant-parameter` text says that the rule reports
  each option key of an object-pattern parameter. The JSON output of the
  commands does not change.
  (Closes [#4846](https://github.com/fallow-rs/fallow/issues/4846))
