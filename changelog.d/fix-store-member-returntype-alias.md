### Fixed

- **`unused-store-member` now resolves imported and later-declared `ReturnType` store aliases.** A helper can read a Pinia store through a parameter typed `CounterStore`, where `type CounterStore = ReturnType<typeof useCounterStore>`. Fallow now credits the member when another file declares and exports the alias, and when the same file declares the alias after the helper. Before this fix, Fallow reported these members as unused. The extraction cache version changes, so the first run after the upgrade parses all files again.
  (Closes [#3357](https://github.com/fallow-rs/fallow/issues/3357))
