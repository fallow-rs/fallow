### Fixed

- **A private class method now takes coverage from its own `#name` record.**
  Before, `fallow health --coverage` always reported a private method
  (`#name() {}`) as `estimated`, also when the coverage data had a record for
  that method. Fallow now uses a record that has the same `#name` near the
  method. The record must be the only one with that name there, so a private
  getter and setter pair stays `estimated`. Fallow does not give a private
  method the coverage of the function that holds it or of a public method.
  istanbul-lib-instrument writes no record for a private method, so its
  reports do not change. Native V8 coverage gets the records when the V8
  converter in `oxc-coverage-instrument` writes them
  ([oxc-coverage-instrument#303](https://github.com/fallow-rs/oxc-coverage-instrument/issues/303)).
  Thanks to @Jerc92 for the report.
  (Refs [#3816](https://github.com/fallow-rs/fallow/issues/3816))
