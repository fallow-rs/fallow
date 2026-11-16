### Added

- **`production.ignore` drops paths from discovery in production mode only.**
  The object form of `production` accepts `ignore`, an array of
  project-root-relative globs, for example
  `"production": { "deadCode": true, "ignore": ["scripts/**"] }`. A run in
  production mode leaves these paths out, in addition to the built-in test,
  spec, story and dev patterns. The default run keeps them. The globs apply to
  each analysis that runs in production mode, also when `--production` or a
  `FALLOW_PRODUCTION*` variable selects the mode. The boolean form of
  `production` does not change.
  (Closes [#4622](https://github.com/fallow-rs/fallow/issues/4622))

### Changed

- **`fallow migrate` maps production exclusions to `production.ignore`.**
  A `!pattern!` entry in a `project` setting of the migrated config, at the
  root or in a workspace, becomes a `production.ignore` glob. Before, `fallow migrate`
  dropped these entries and suggested an `overrides` entry, which hid the
  findings in every mode.
