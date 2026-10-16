### Fixed

- **`fallow dupes` reports clones inside an object that a call receives.**
  Before, a copied block in an object literal argument was not reported when
  the call had no callback argument. Examples are the computed properties and
  methods in `defineComponent({ ... })` or in a wrapper with the same shape.
  Fallow treated the full call as repeated call syntax and filtered the clone.
  Now a call that holds a function at any depth, such as an object method, an
  arrow function in a nested object, or an immediately invoked function, is
  not filtered. A call with only data arguments is still filtered. The
  duplication cache version changes, so the first `fallow dupes` run after the
  upgrade tokenizes the project again. Thanks
  [@jleifeld](https://github.com/jleifeld) for the report.
  (Closes [#3491](https://github.com/fallow-rs/fallow/issues/3491))
