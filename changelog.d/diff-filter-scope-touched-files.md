### Fixed

- **A pure-rename diff no longer gets an empty-scope warning beside a finding.** `request_outcomes.diff-filter.scope_size` counted only added lines. A diff that renames a file and adds no line gave `0`, so the GitHub Action, the GitLab template, the MCP tools and the request-outcome text said that no finding could survive. The diff filter keeps a finding on a whole touched file, so the report still held, for example, the renamed unused file. `scope_size` now also counts each touched file that adds no line. It is `0` only when the diff touches no file and adds no line.
  (Closes [#4974](https://github.com/fallow-rs/fallow/issues/4974))
