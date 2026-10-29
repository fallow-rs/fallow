### Fixed

- **An `unused-enum-member` suppression that only `--production` needs is no
  longer reported stale in a run without `--production`.** Production mode
  excludes test files. An enum member that only a test file reads is unused
  there, so its `fallow-ignore-next-line unused-enum-member` comment is
  necessary. A run without `--production` now scans the enum members again
  without the files that production mode excludes. It keeps the comment out of
  `stale-suppression` when that scan reports the member. The comment stays
  stale when a production file reads the member. Thanks @Smrtnyk for the
  report.
  (Closes [#3962](https://github.com/fallow-rs/fallow/issues/3962))
