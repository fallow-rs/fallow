### Fixed

- **Glob expansion on disk works when the project path contains `[`, `*` or
  `?`.** Plugin `fileExists` detection, `fallow plugin-check` entry seeds and
  `package.json` or `pnpm-workspace.yaml` workspace globs put the project
  root into the glob without an escape. Under a root such as
  `~/work/app [1]`, these globs then matched nothing. A plugin did not
  activate, and a workspace package was not found, so its files were
  reported as unused. Tooling config detection
  already escaped the root. All four now use one function that escapes the
  root and keeps every match inside it.
