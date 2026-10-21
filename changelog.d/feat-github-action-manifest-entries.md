### Added

- **Fallow reads the entry files of a JavaScript GitHub Action.** When
  `action.yml` or `action.yaml` sets `runs.using` to a `node` runtime, the
  `runs.pre`, `runs.main` and `runs.post` files are entry points. A path in
  `dist/` maps to its source file with the same rules as a `package.json`
  `main` field. A composite or Docker action adds no entry points.

### Fixed

- **VitePress: the `vite` block of the VitePress config credits its
  packages.** Packages in `vite.optimizeDeps`, `vite.ssr.external` and
  `vite.ssr.noExternal` are no longer unused dependencies, and
  `vite.resolve.alias` entries resolve imports.
- **Vite: a nested `optimizeDeps` entry credits its parent package.** An entry
  such as `'mermaid > @braintree/sanitize-url'` now credits `mermaid`.
