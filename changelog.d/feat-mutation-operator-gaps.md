### Added

- **`fallow mutation` makes more mutants with the existing operators.** These rules close gaps against the StrykerJS mutator set:
  - an `if` or `? :` test that is a comparison or a logical expression now becomes `true` and `false`. A loop test of this kind becomes `false` only, and `for (;;)` gets the test `false`;
  - an operand of `&&` that is a comparison or a `!x` becomes `true`, and such an operand of `||` becomes `false`;
  - an equality also becomes its other constant: `a === b` becomes `true` as well as `false`;
  - `regex-literal` removes a `+`, `*` or `?` quantifier, shortens `{n,m}` to `{n}`, and removes an anchor at the start or end of a group or an alternative;
  - `return-value` changes the concise body of an arrow function to `undefined`;
  - `arithmetic-operator` swaps `*=` and `/=`, `<<=` and `>>=`, `&=` and `|=`, and changes `%=` to `*=`. `logical-operator` swaps `&&=` and `||=`, and changes `??=` to `&&=`;
  - `unary-operator` swaps `-x` and `+x`, and removes `~`;
  - `string-literal` empties a template literal and fills an empty one. A tagged template gives no mutant.

  The operator names do not change. An existing mutant keeps its id. A mutant that two rules make is one mutant.

### Changed

- **`arithmetic-operator` skips `*`, `/` and `%` with a string or template operand**, as it already did for `+` and `-`.
