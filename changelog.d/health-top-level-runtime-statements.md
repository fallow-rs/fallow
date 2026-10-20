### Fixed

- **`fallow health` counts runtime code in modules that export only types.**
  Top-level calls, assignments, `await`, and runtime declarations keep a module
  in coverage gaps. Pure type and ambient declarations remain excluded. The
  extraction cache version is bumped so warm caches pick up the new facts.
  (Closes [#3549](https://github.com/fallow-rs/fallow/issues/3549))
