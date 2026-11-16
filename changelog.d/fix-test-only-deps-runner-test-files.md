### Fixed

- **`test-only-dependencies` counts the test files that a test runner config selects.** A Vitest `include`, a Jest `testMatch` or a Playwright `testDir` can name test files that the built-in name patterns do not match, such as `*.vitest.ts`. The rule treated these files as production code, so a package that only they import stayed in `dependencies` without a finding. A test entry point of a test runner now counts as a test file, unless a runtime entry point reaches it. A `framework` entry with `"entryPointRole": "test"` also counts.
  (Closes [#4618](https://github.com/fallow-rs/fallow/issues/4618))
