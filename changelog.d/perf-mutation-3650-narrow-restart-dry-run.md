### Performance

- **`fallow mutation` no longer repeats the setup of unrelated test files for
  each static mutant with Vitest `isolate: false`.** Without fresh modules
  for each run, each static mutant runs in a new runner process. That process
  did a dry run of each test file of the run. It now dry-runs only the test
  files that cover the mutant. The safety check of the new process stays: a
  covering test that does not pass in its dry run is still a baseline failure.
  (Closes [#3650](https://github.com/fallow-rs/fallow/issues/3650))
