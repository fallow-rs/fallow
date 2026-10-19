### Added

- **`fallow audit` reports inherited complexity findings that got worse.** A
  complexity finding that was already in the base snapshot is now `worsened`
  when a measured value is higher than at the base: cyclomatic, cognitive,
  CRAP (when both sides have a score) or the line count of the function. A
  function that grows from 80 to 300 lines, but keeps its complexity values,
  no longer passes as a plain inherited finding. Each complexity finding in
  the JSON output carries `attribution` (`introduced`, `worsened` or
  `inherited`) and `worsened_metrics` with the base value and the head value,
  and `attribution.complexity_worsened` counts the worsened findings. The
  human, compact and markdown output add a "Worse than base" section, SARIF
  puts the values in the result property bag, and CodeClimate descriptions
  name them. The new `--worsened` flag and the `audit.worsened` config key
  (`warn` or `error`) decide the effect: `warn` (default) makes the verdict at
  least `warn` and keeps exit code 0, `error` counts a worsened finding like
  an introduced finding. The MCP `audit` tool takes a `worsened` parameter. A
  finding whose exceeded metric grew stays introduced and gates in both modes,
  as before.
