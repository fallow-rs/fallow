### Changed

- **`fallow audit --gate new-only` reports growth of an already exceeded
  complexity metric as `worsened`, not `introduced`.** A function that
  exceeded a threshold on the base and grows a little in the PR no longer
  fails the audit by default. With the default `audit.worsened: warn`, the
  verdict is `warn` and the exit code is 0. The finding stays `introduced`
  when the growth moves it to a higher severity tier (`moderate`, `high`,
  `critical`), or when it now exceeds a metric that it did not exceed on the
  base and that metric grew. The exceeded flags and the tier of the base
  finding come from the base run, so a threshold override in the same PR does
  not make an old finding introduced.

  To keep the earlier gate, set `audit.worsened: "error"` or pass
  `--worsened error`. With `audit.worsened: off`, growth of an exceeded metric
  now gives a `warn` verdict as `worsened`, and the run does not fail. Growth
  only in metrics that the finding does not exceed stays `inherited`, as
  before. The JSON fields do not change. Cached base snapshots of earlier
  versions are not reused.
