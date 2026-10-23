### Fixed

- **A suppression that only `--production` needs is no longer reported stale
  in a run without `--production`.** Production mode excludes test files. An
  export that only a test file imports is unused there, so its
  `fallow-ignore-next-line unused-export` comment is necessary. A run without
  `--production` credits the test import and does not use the comment. That
  run now keeps the comment out of `stale-suppression` when every reference to
  the export comes from a file that production mode excludes. The comment
  stays stale when a package entry that production mode keeps re-exports the
  export. This applies to `unused-export` and `unused-type`. Thanks to
  @Smrtnyk for the report.
  (Closes [#3825](https://github.com/fallow-rs/fallow/issues/3825))
