### Fixed

- **A package script with thousands of test arguments no longer exhausts
  memory or stalls.** Fallow turns each directory argument of `node --test`,
  `bun test` and `deno test` into default test file patterns. A script with
  88,000 such arguments made millions of patterns. Fallow now expands each
  directory once and at most 64 directories for each command. The check of
  test files against flag values is now linear. A chain of command wrappers,
  such as `vpx vpx vpx ...`, now stops after 8 wrappers, because each wrapper
  copied every argument after it. Found by OSS-Fuzz.
