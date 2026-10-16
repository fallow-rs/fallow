### Fixed

- **An `.extend<T>(...)` on an imported Playwright test credits the fixture
  class members that tests use.** Before, in
  `export const bTest = appTest.extend<{ b: BPage }>({})` with `appTest`
  imported from another file, Fallow dropped the fixtures of `T`. The `BPage`
  methods that tests call through the `b` fixture then reported as unused
  class members. Now Fallow keeps these fixtures and applies them when
  `appTest` resolves to a Playwright test. This also covers a base made by an
  untyped `base.extend({})`, a re-exported base, a chain of such files, and a
  helper that returns the `.extend<T>(...)` call. An `.extend<T>(...)` on an
  imported value that is not a Playwright test still credits nothing. The
  extraction and graph cache versions change, so the first run after the
  upgrade parses the project again.
