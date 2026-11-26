### Fixed

- **An unknown key in a nested config section now stops the run with exit code 2.** The top-level config and `boundaries` already rejected an unknown key. These sections ignored one: `duplicates`, `duplicates.normalization`, `flags`, `flags.sdkPatterns`, `health.ownership`, `resolve`, `fix`, `fix.catalog`, `audit`, `regression` and the object form of `ignoreExportsUsedInFile`. A misspelled key such as `minToken` (for `minTokens`) then turned off a setting without a message. A snake_case key such as `min_tokens` had the same effect.
  - A config with a stale or misspelled key in one of these sections that ran before now fails. Correct or remove the key.
  - `regression.baseline` still accepts an unknown key, because a newer fallow can write a count that an older one does not know.
  - A nested key that a later fallow version adds now needs `minimumVersion` in a shared config, as a new top-level key does.
- **An unknown-key error now names the valid key that is closest to the unknown key.** For example, the error for `minToken` asks: did you mean `minTokens`? The error for the object form of `ignoreExportsUsedInFile` now names the unknown key.
