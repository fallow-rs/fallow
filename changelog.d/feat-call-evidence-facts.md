### Changed

- **The extraction cache now holds the parameters of module-scope functions and the arguments of their direct calls.** A later opt-in rule reads these facts to report a parameter that gets the same value at each call. No report changes. The first run after the upgrade parses each file again, because the cache format changed.
  (Closes [#4464](https://github.com/fallow-rs/fallow/issues/4464))
