### Fixed

- **`fallow mutation` no longer makes an equivalent `true` mutant for an
  optional lookup before `.filter(Boolean)`.** In
  `["base", align ? alignment[align] : ""].filter(Boolean)`, the constant
  `true` for `align` gives `undefined` instead of `""` for a falsy `align`,
  and `filter(Boolean)` drops both. No test can kill that mutant. The
  `false` mutant stays. The object must be a local `const` object literal
  that is not exported, has no falsy key such as `undefined` or `0`, and is
  only read. A template literal, a `.join` without `.filter(Boolean)` and a
  truthy alternate keep both mutants.
  (Closes [#3667](https://github.com/fallow-rs/fallow/issues/3667))
