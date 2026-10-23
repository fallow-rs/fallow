### Changed

- **Fallow now parses with Oxc 0.155 and converts coverage with `oxc_coverage_instrument` 0.17.0.**
  A short TypeScript input that made the parser use seconds of time and
  gigabytes of memory (oxc-project/oxc#27637) now stops at its syntax error at
  once. Analysis output for real projects does not change.

### Fixed

- **An executed private class method now gets its measured coverage from native V8 coverage.**
  Before, `fallow health --coverage <NODE_V8_COVERAGE dir>` reported a private
  method (`#name() {}`) as `estimated` and gave it a CRAP finding, also when
  Node ran every path of the method. Its public twin with the same body was
  measured and had no finding. The V8 converter now writes a record for each
  private method (`#name`, `get #name`, `set #name`) with its own call count,
  and Fallow uses that record. A private method that did not run stays at 0%.
  Thanks to @Jerc92 for the report.
  (Closes [#3816](https://github.com/fallow-rs/fallow/issues/3816))
