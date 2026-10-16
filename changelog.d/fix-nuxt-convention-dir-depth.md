### Fixed

- **Nuxt nested plugin, middleware and module files are now reported when
  unused.** Nuxt scans `plugins/`, `middleware/` and `modules/` one level
  deep: a direct child file, or the `index` file of a direct child directory.
  Before, Fallow made every file in these directories an entry point at any
  depth. An unused helper such as `plugins/foo/helper.ts` or
  `middleware/admin/guard.ts` was then not reported. Now these files must be
  imported to count as used. `plugins/bar.ts`, `plugins/foo/index.ts`, the
  `plugins` list of `nuxt.config`, `srcDir` and local layers keep their
  credit. The files in `modules/<name>/runtime/` stay entry points, because a
  module registers them through `resolver.resolve()` strings. Middleware and
  module files with the `tsx`, `jsx`, `mts`, `cts`, `mjs` and `cjs`
  extensions are now entry points too, as Nuxt scans them.
