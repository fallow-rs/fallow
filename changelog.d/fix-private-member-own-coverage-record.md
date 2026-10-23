### Fixed

- **A private class method now takes coverage from its own `#name` record.**
  Before, `fallow health --coverage` always reported a private method
  (`#name() {}`) as `estimated`, also when the coverage data had a record for
  that method. Fallow now uses a record that has the same `#name` near the
  method. A record at the exact position of the method is used first. Near
  the method, the record must be the only one with that name. Fallow does not
  give a private method the coverage of the function that holds it or of a
  public method. istanbul-lib-instrument writes no record for a private
  method, so its reports do not change.
  Thanks to @Jerc92 for the report.
  (Refs [#3816](https://github.com/fallow-rs/fallow/issues/3816))
