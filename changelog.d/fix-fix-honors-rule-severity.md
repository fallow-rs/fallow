### Fixed

- **`fallow fix` no longer changes a finding that an `overrides` entry turns `off`.** The reports leave out a finding when an `overrides` entry sets its rule to `off` for its file, but `fix` planned and applied a fix for it. For example, with `unused-exports` set to `off` for `src/jobs/**`, `fallow dead-code` reported no export there while `fallow fix --yes` removed the `export` keyword of each unused one. `fix` now runs the same rule pass as the reports, so it changes exactly the findings that `fallow dead-code` lists.
