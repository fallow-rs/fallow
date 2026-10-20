### Fixed

- **`fallow mutation` now runs the tests of static importers for a module
  that a test also imports at run time.** When one test file imports a module
  statically and another test loads it through a dynamic `import()`, a mutant
  in module-level code ran only the dynamic test. The assertion of the static
  importer never ran, and the mutant survived. The mutant is now a static
  mutant and runs the tests of both files.
  (Closes [#3652](https://github.com/fallow-rs/fallow/issues/3652))
