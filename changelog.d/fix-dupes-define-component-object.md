### Fixed

- **`fallow dupes` reports clones inside an object that a call receives.**
  Before, a copied block in an object literal argument was not reported when
  the call had no callback argument. Examples are the `props`, computed
  properties, and methods in `defineComponent({ ... })` or in a wrapper with
  the same shape. Fallow treated the full call as repeated call syntax and
  filtered the clone. Now fallow reports the clone when the call holds a
  function at any depth, such as an object method or an arrow function in a
  nested object. Fallow also reports the clone when it starts inside an object
  or array literal argument, because then it does not hold the callee. A
  copied `props` block with only data is an example. A repeated call whose
  clone also covers the callee and the argument list is still filtered.
  A method chain is not filtered when an earlier call in the chain has a
  callback, for example `xs.map((x) => x.id).filter(Boolean)`. Before, fallow
  filtered the full chain when the last call had no callback, so you can see
  new clone groups in chained code. The duplication cache version changes, so
  the first `fallow dupes` run after the upgrade tokenizes the project again.
  Thanks [@jleifeld](https://github.com/jleifeld) for the report.
  (Closes [#3491](https://github.com/fallow-rs/fallow/issues/3491))
