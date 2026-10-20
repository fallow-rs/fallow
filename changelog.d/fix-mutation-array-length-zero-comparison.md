### Fixed

- **`fallow mutation` no longer makes equivalent mutants for a zero
  comparison on the length of a known array.** An array length is never
  negative, so `items.length > 0` and `items.length != 0` give the same
  result. No test can kill that mutant. The rule also covers `<= 0` (the
  `==` swap), `< 0` (the constant `false`), `>= 0` (the constant `true`) and
  the reversed forms. The receiver must be an array literal, or a name
  that resolves to a `const` with an array value, a binding with an array
  type, or a rest parameter. A global with the same name and an object with
  a `length` property keep each mutant.
  (Closes [#3651](https://github.com/fallow-rs/fallow/issues/3651))
