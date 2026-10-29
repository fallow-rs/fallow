### Fixed

- **Mutation testing instruments temporary Node source copies.** Byte-identical
  copies of a unique planned source load its instrumented code. Copies that
  match several planned sources make the run incomplete, even when a test
  ignores the child result.
  (Closes [#3950](https://github.com/fallow-rs/fallow/issues/3950))
