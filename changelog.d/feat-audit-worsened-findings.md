### Added

- **`fallow audit` reports inherited complexity findings that got worse.** A
  complexity finding that is in the base snapshot is now `worsened` when a
  measured value is higher than at the base. The measured values are
  cyclomatic, cognitive, CRAP (when both sides have a score) and the line
  count of the function. The line count must grow by at least 5 lines and at
  least 10 percent. The line count includes comments and blank lines. The
  minimum keeps a finding inherited after a short comment or a formatter
  rewrap. A function that grows from 80 to 300 lines with the same complexity
  values is now `worsened`, not inherited.

  The JSON output adds these fields:

  - `attribution` on each complexity finding: `introduced`, `worsened` or
    `inherited`.
  - `worsened_metrics` on each complexity finding with a grown value: the
    metric, the base value and the head value.
  - `attribution.worsened`: the active mode.
  - `attribution.complexity_worsened`: the number of worsened findings.

  The human, compact and markdown output add a "Worse than base" section.
  SARIF puts the values in the result property bag. CodeClimate descriptions
  name them. The GitHub summary marks each finding as `worsened`.

  The `--worsened` flag and the `audit.worsened` config key select the effect:

  - `warn` (default): a worsened finding makes the verdict at least `warn`.
    The exit code stays 0.
  - `error`: a worsened finding counts like an introduced finding.

  The GitHub Action takes a `worsened` input. The GitLab template takes
  `FALLOW_AUDIT_WORSENED`. The MCP `audit` tool takes a `worsened` parameter.
  A finding whose exceeded metric grew stays introduced and gates in both
  modes, as before.
