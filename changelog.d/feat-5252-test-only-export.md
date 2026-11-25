### Added

- **A `test-only-export` rule.** It reports an exported value that production code does not use while a test, story, fixture, bench or test-helper file does. In the default mode the tests keep such an export alive, so `unused-export` does not report it. The finding lists up to five `test_importers` and sets `colocated_only` when each importer is next to the module. Its actions are `remove-export-and-tests` and `test-through-interface`, both manual, and `suppress-line`. The rule is `off` by default and runs only in a default-scope run; a `--production` run reports the same export as `unused-export`. Only values are checked in this version, not types. Turn it on with `rules: { "test-only-exports": "warn" }` or the `--test-only-exports` selector, and read `fallow explain test-only-export`.
  (Closes [#5252](https://github.com/fallow-rs/fallow/issues/5252))

### Fixed

- **An `@expected-unused` export that only tests import is no longer a stale suppression.** A default-scope run gave a `stale-suppression` finding for it, although production code does not use the export. The tag stays stale when a production file uses the export.
  (Closes [#5252](https://github.com/fallow-rs/fallow/issues/5252))

### Changed

- **The extraction cache version changes.** A cached file that has a `// fallow-ignore-next-line test-only-export` comment held the token as unknown, so the first run after the upgrade parses each file again.
