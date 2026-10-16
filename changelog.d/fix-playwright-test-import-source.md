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
- **A project that depends only on `playwright` and has a Playwright config
  activates the Playwright plugin.** Before, only `@playwright/test` activated
  it, so the spec files of such a project reported as unused files. The
  `playwright` package alone still does not activate the plugin without a
  `playwright.config.ts` or `playwright.config.js` file, because Vitest and
  Rstest browser modes use it without the test runner.
