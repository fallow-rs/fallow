### Changed

- **The blast-radius `risk_band` follows the importer count alone.** `high` at 20 or more importer files, `medium` at 5 or more, else `low`. Traffic no longer moves the band, in `coverage analyze --cloud` and in `health --runtime-coverage`. This is the rule of Fallow Cloud, so one function gets one band in every output. A cloud row without a caller count that fallow cannot fill now reads `unknown`, the value that the cloud sends, not `low`.

- **`caller_count_weighted_by_traffic` has one meaning and can be `null`.** It is the sum of the production hits in the importer files of the function, as Fallow Cloud measures it. Before, rows that fallow filled from the local AST held the own invocations times the callers. The value is `null` when no importer file has runtime data, and in a local `health --runtime-coverage` run, which has no measure for it. Before, a missing value read as `0`.
  (Refs [fallow-rs/fallow-cloud#1642](https://github.com/fallow-rs/fallow-cloud/issues/1642))
