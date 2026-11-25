### Fixed

- **`fallow mutation` keeps pruned mutants out of the instrumented copy.** Each pruned mutant, for example a `framework-config` mutant, now keeps its original text in the copy that the runner loads. Before, the Vitest overlay served the copy to each build plugin, and the TanStack Router code splitter failed the baseline with `You must provide a string literal for the codeSplitGroupings`. The ids of the other mutants do not change.
  (Closes [#5218](https://github.com/fallow-rs/fallow/issues/5218))
