### Fixed

- **`fallow health --production --runtime-coverage` no longer reports tested functions as `not_covered`.**
  The fix for #3822 gave the CRAP estimate and the coverage gaps the test files
  in production mode. The runtime coverage evidence still used the production
  graph, which has no test files, so `evidence.test_coverage` was
  `not_covered` for each function. The runtime coverage evidence now reads
  test reachability from the same full-project graph. A production health run
  builds this graph one time and uses it for file scores and for runtime
  coverage. A production run with runtime coverage and without file scores
  (for example `--complexity` with `maxCrap: 0`) now builds this graph too, so
  it takes more time.
