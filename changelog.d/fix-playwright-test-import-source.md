### Fixed

- **A Playwright `test` or `mergeTests` import from `playwright/test` credits
  the fixture class members that tests use.** The `playwright` package
  exports the same test runner at `playwright/test` that `@playwright/test`
  re-exports. Before, Fallow read Playwright fixtures only from the
  `@playwright/test` source. A fixture test made with `test.extend<T>(...)` or
  `mergeTests(...).extend<T>(...)` from `playwright/test` lost its fixtures,
  so the page-object methods that tests call through them reported as unused
  class members. Now both sources work the same. The extraction and graph
  cache versions change, so the first run after the upgrade parses the
  project again.
