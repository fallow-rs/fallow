### Changed

- **Help and schema texts use plain case.** Option help, config schema
  descriptions and output schema descriptions no longer use all-caps words
  for emphasis. The `includeEntryExports` description now says that fallow
  checks the other exports of an entry file when a plugin `usedExports` rule
  matches the file, also without the setting. The unused class member
  heading now says that a use through `this` counts as a use. The
  environment registry of `fallow schema` gives the default `2.85.0` for
  `FALLOW_GATE_MIN_VERSION`.
- **`fallow explain` says what goes wrong when a finding stays.** The "Why it
  matters" section of dead-code, dependency, component, suppression, styling,
  test reachability, duplication, runtime coverage and flag rules no longer
  repeats the definition of the rule. Each tainted-sink security category has
  its own reason and its own example line with a sink of that category. The
  three complexity rules, `untested-file` and `untested-export` have one
  example each, and `feature-flag` and `flag-retirement-candidate` have an
  example and a fix. The SARIF `fullDescription` of each rule does not change.

### Fixed

- **The human duplication report uses the singular for one clone group.**
  The header reads `Duplicates (1 clone group)`. The overflow and corpus
  total lines also use the singular for one group or one family.
- **The audit scope line starts the range at the base commit.** `fallow
  audit` printed the head commit as the start of the range, so the line read
  as an empty range. The line now prints the merge base of the base ref and
  `HEAD`, the commit that the changed-file diff starts from.
  (Closes [#4838](https://github.com/fallow-rs/fallow/issues/4838))
- **The commit gate reads only the first line of `fallow --version`.** The
  npm launcher prints a second `verified:` line, which the gate put into its
  block message and its version check.
  (Closes [#4842](https://github.com/fallow-rs/fallow/issues/4842))
