### Fixed

- **`fallow health --save-snapshot` keeps each snapshot of one second.** The default file name has whole seconds, so a second save in the same second replaced the first snapshot. A later save now gets a numeric suffix, for example `2026-01-01T00-00-00Z-2.json`.

- **`fallow health --hotspots --ownership` reads the configured `codeowners` path.** The hotspot ownership probed only the standard CODEOWNERS locations, so `declared_owner` and `unowned` ignored a file at a custom path. `--group-by owner` and the review brief already read that path.

- **Bare `fallow` measures scoped health duplication like `fallow health`.** When `health.ignore`, a workspace or a changed-files scope narrowed the health files, the combined run cut the project duplication report down to the scope. That kept clone pairs that `duplicates.minOccurrences` drops and counted the totals over other files, so `duplication_pct` and the score differed from standalone `fallow health`. The combined run now runs the scoped detection in that case.
