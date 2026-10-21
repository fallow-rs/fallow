### Fixed

- **`fallow mutation` now stops a Vitest inline project at the first failed
  test.** The Vitest companion set fail-fast only on the root config. A
  worker reads the config of its project, and an inline project in
  `test.projects` keeps the `bail` value that Vitest copied at start. A
  mutant run in an inline project then ran each selected test after the
  first failure. The companion now sets `bail` on each project for each run,
  and a control run still runs the full selection.
  (Closes [#3701](https://github.com/fallow-rs/fallow/issues/3701))
