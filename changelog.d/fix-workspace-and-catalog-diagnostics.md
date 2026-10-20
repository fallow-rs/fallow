### Added

- **New workspace diagnostic `workspace-pattern-dir-missing`.** A positive
  workspace pattern whose directory does not exist (for example `vite` and
  `vite/packages/*` for a repository that CI clones into the workspace) now
  gets a diagnostic with the pattern and the declaring file. The packages in
  that directory can use catalog entries, so the run does not report
  `unused-catalog-entries` while such a pattern exists. Unresolved catalog
  references and empty catalog groups are still reported.
- **New workspace diagnostic `nested-project-root`.** A directory with its own
  `package.json` and `pnpm-workspace.yaml` that no root workspace pattern
  declares is an independent pnpm project. Fallow now analyzes it as a
  workspace and reports this information-only diagnostic.

### Fixed

- **A `package.json` with no name and no dependencies is not an undeclared
  workspace.** A file such as `bench/package.json` with only
  `{"type":"module"}` sets the module type of the scripts next to it. It no
  longer gets `undeclared-workspace`.
- **A nested independent pnpm project owns its files and dependencies.** Its
  `package.json` now activates framework plugins and declares the dependencies
  of its files, without a `workspaces.patterns` entry in the fallow config. Its
  directories are not reported as undeclared workspaces, and its `catalog:`
  references are not checked against the root catalog.
- **pnpm overrides take precedence over a `catalog:` reference.** pnpm applies
  an override before it resolves `catalog:`. A `package.json` reference to a
  package that an override key with no version selector replaces (`vite`, or
  `<consumer>>vite`) is no longer an unresolved catalog reference. A key with a
  version selector (`vite@^5`) does not replace the reference, because pnpm
  fails the install for it.
