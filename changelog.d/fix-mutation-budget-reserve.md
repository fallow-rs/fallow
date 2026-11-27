### Fixed

- **`fallow mutation --time-budget` uses its budget.** The first pass keeps time free for the retries of its survivors and timeouts. That reserve held the time of the full first dry run, but a retry companion dry-runs only the test files of the suspects. So a run ended early by about the time of the first dry run, with mutants `pending`. The reserve now holds the setup time plus the dry-run time of the test files of the suspects. Each suspect still gets its retry before the budget ends.
  (Closes [#5421](https://github.com/fallow-rs/fallow/issues/5421))
