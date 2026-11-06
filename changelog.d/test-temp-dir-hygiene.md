### Changed

- **The JS test suites no longer leave `fallow-*` folders in the system temp directory.** The mutation runner tests give each companion a private temp directory and remove it when the companion ends, as the mutation API does. The script tests create their temp folders through `scripts/lib/temp-dir.mjs`, which removes them when the tests of the file end. The new `scripts/check-temp-leaks.mjs` runs a command and fails when the command leaves a new `fallow-*` entry in the temp directory. CI runs each JS test suite through it. This change affects contributors only.
