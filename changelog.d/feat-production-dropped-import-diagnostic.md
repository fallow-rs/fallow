### Added

- **Production mode reports a test pattern that drops a file that production code imports.**
  A built-in production pattern such as `**/test/**` also matches production
  source, for example `src/components/runner/test/Runner.vue`. Fallow does not
  analyze that file, so an export or a parameter that only this file uses can
  appear in a finding. When a production file imports such a file,
  `workspace_diagnostics[]` now carries one `production-excluded-file-imported`
  entry for each pattern, with `pattern`, `file_count` and the first five
  `files`. The human output prints the same warning on stderr. The production
  patterns do not change. To analyze the file, move it out of the matched path,
  or run that analysis without production mode (`--no-production` or
  `production: { deadCode: false }`).
  (Closes [#4498](https://github.com/fallow-rs/fallow/issues/4498))
