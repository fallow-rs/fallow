### Fixed

- **`fallow audit` counts `@effect/vitest` tests.** The `weakened-tests (removed-tests)` check now counts `it.effect`, `it.live`, `it.scoped`, `it.scopedLive` and `it.prop` as test cases. It also counts chains such as `it.effect.skip`, `it.effect.each` and `it.effect.prop`. A move from `it(...)` to `it.effect(...)` no longer gives a removed-tests finding. `it.layer` and `it.flakyTest` do not register a test, so the check does not count them.
  (Closes [#4095](https://github.com/fallow-rs/fallow/issues/4095))
