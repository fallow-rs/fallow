### Fixed

- **CLI texts match the behavior.** The help and the input error of `fallow dupes --trace` name
  both accepted forms, `FILE:LINE` and `dup:<fingerprint>`. The
  `code-duplication` explanation describes each `duplicates.mode` value
  correctly: `weak` ignores only string values, and only `semantic` ignores
  identifiers. The combined report prints no em dash in its target hint and
  in the warning for a project without entry points.
  (Closes [#4830](https://github.com/fallow-rs/fallow/issues/4830))
