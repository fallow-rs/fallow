### Fixed

- **`ignore-dependencies-entry-redundant` no longer tells you to remove an entry that a `--production` run needs.** A normal run could call an `ignoreDependencies` entry redundant when the entry hid a `type-only-dependencies` or `unused-dependencies` finding in a production run, for example a package with only `import type` in source files, or a package that only tests or non-production scripts use. A normal run now leaves out such an entry. Findings and exit codes do not change.
