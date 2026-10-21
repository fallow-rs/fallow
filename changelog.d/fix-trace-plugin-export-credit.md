### Fixed

- **`--trace` no longer calls an export unused when a framework plugin
  marks it as used.** The trace read only import references. The `default`
  export of a Next.js page or the export that a Serverless handler names
  traced as `is_used: false`, although the analysis did not report it.
  `--trace` and `--trace-file` now list these credits in a new
  `plugin_credits` field with the plugin and its file pattern. For a
  reachable file, the export then traces as used, and the reason names the
  plugin. The human output shows the same credit.
