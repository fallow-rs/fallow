### Added

- **`--lean` trims the JSON report for agent context.** The new global flag
  works with `--format json` on `fallow dead-code`, `fallow health` and the
  bare `fallow` run. A lean document leaves out empty arrays and zero
  dead-code summary counts, and keeps one workspace diagnostic per kind, with
  a `count` when the kind has more than one. It keeps the first 10 rows of
  the health tables `hotspots`, `file_scores` and `large_functions`, and
  `<table>_omitted` gives the number of rows left out. `--full-tables` keeps
  every row. `fallow health --top` cuts the tables as before. Repeated action text moves to `_meta.actions`, and an action without a
  text field reads it there. The root field `profile: "lean"` marks the
  shape, which `docs/output-schema.json` describes as `LeanOutput`. Runs
  without `--lean` write the full shape as before. Other commands reject the
  flag, and `fallow report --from` refuses a lean document.
  (Closes [#3992](https://github.com/fallow-rs/fallow/issues/3992))

### Changed

- **`--explain` lists only the rules of the reported findings.** `_meta.rules`
  in dead-code, architecture and combined JSON now holds only the rule codes
  that have a finding in the run, and a run without findings has no `rules`
  key. The CLI, the MCP server and the programmatic API list the same rules.
  `fallow explain <rule>` still gives the text of any rule.
  (Closes [#3992](https://github.com/fallow-rs/fallow/issues/3992))
