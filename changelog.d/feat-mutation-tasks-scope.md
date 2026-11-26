### Added

- **`fallow mutation tasks --format json` gives the scope of the run.** The new `scope` object has the `kind` of the run (`changed`, `all` or `paths`), the `base_ref` of a `changed` run, and `files`: each file with a mutant result in the report, also a file without a task. A consumer can now tell a mutant that a test killed from a mutant that the run did not mutate, and does not need to read the saved report.
  (Closes [#5289](https://github.com/fallow-rs/fallow/issues/5289))
