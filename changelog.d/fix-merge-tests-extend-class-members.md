### Fixed

- **A chained `mergeTests(...).extend(...)` Playwright test credits the
  fixture class members that tests use.** Before, Fallow read the receiver of
  `.extend(...)` only when it was an identifier. In
  `export const test = mergeTests(appTest).extend({})`, the merged fixtures
  and the fixtures of an `.extend<T>(...)` type argument were lost, so the
  page-object methods that tests call through them reported as unused class
  members. Now the chained form works the same as the form with an
  intermediate variable. This also covers an aliased `mergeTests` import and a
  helper that returns the chain. A chain of more than one `.extend<T>(...)`
  call, for example `base.extend<A>({}).extend<B>({})`, defines the fixtures
  of each type argument. An `.extend<T>(...)` on a local const that
  holds a merged or extended Playwright test now also defines the fixtures of
  `T`. The extraction and graph cache versions change, so the first run after
  the upgrade parses the project again. Thanks to @vethman for the report.
  (Closes [#3462](https://github.com/fallow-rs/fallow/issues/3462))
