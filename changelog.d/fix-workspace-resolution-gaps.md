### Fixed

- **A `<workspace-package>/package.json` import resolves without `node_modules`.**
  Fallow reported the import as unresolved and the workspace dependency as
  unused when the package was not installed. The import now credits the
  package, the same as an installed workspace package. An `exports` map
  without a `./package.json` key still blocks the import.
- **`require.resolve` on a function-scoped `createRequire(import.meta.url)`
  credits the package.** Fallow treated the local `require` as a shadow and
  reported the dependency as unused. A `require` from
  `createRequire(import.meta.url)` or `createRequire(__filename)` now counts in
  every scope.
- **`require('..')` and `require('.')` resolve to package source.** A relative
  import of a package root whose `main` points at missing build output now uses
  the source entry, the same as `require('../')`.
