### Added

- **`fallow mutation` now mutates fluent chains in module-level `const`
  initializers.** The new `removed-chain-link` operator removes one call link
  of a chain, so `z.number().int().positive()` gives a mutant without
  `.int()` and a mutant without `.positive()`. A validation schema that has no
  literal or operator now has mutants. The operator is on by default and
  `--operators` and the `mutation.operators` config accept its name.
  ([#3662](https://github.com/fallow-rs/fallow/issues/3662))
