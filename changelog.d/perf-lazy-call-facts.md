### Changed

- **A run with the `constant-parameter` rule off no longer pays for its call facts in the parse cache.** The cache stored the call facts of each module for every run, so each warm run decoded them, also when no rule read them. Now only a run with the rule on, from the config, `--constant-parameters`, the API filter or `fallow mutation` with `negate-condition`, stores the facts and decodes them. The cache stores them as a separate blob, and a run with the rule off only copies that blob. A run with the rule on parses again each file whose cache entry has no facts, and it gives the same findings as before. The cached extraction format changes, so `CACHE_VERSION` goes to 437.
  (Closes [#4524](https://github.com/fallow-rs/fallow/issues/4524))
