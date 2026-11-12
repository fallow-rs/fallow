### Fixed

- **A dependency that only a framework or build config file imports is no longer reported as test-only.** Fallow counted every config file as a test file. So `astro`, `@astrojs/react` and other packages that only `astro.config.mjs` imports were reported as `test-only-dependencies`. These config files configure the production build. Now only test files and test runner config or setup files (`vitest.config.*`, `jest.setup.*`, `playwright.config.*`, `.mocharc.*`) count as test files. A dependency that only test files import is still reported.
