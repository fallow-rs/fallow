### Added

- **Fallow now reads Vite+ projects.** A `vite-plus` dependency, or a root
  `vite.config.*` that imports `vite-plus`, enables the new `vite-plus`
  plugin. The `pack.entry` values are entry points, and the `lint.jsPlugins`,
  `lint.overrides[].jsPlugins` and `lint.extends` packages are used. A
  `vite.config.*` that lists `@e18e/eslint-plugin` in `lint.jsPlugins` no
  longer reports it as an unused dev dependency. The `vite-plus` dependency
  also enables the Vitest plugin, so the test and bench files are entry
  points without a Vitest dependency.
- **Vite+ commands credit their dependencies.** The `vp`, `vpr` and `vpx`
  binaries credit `vite-plus`. `vp lint` and `vp fmt` file targets are not
  entry points, `vp test -c <file>` reads the file as a Vitest config,
  `vp exec <binary>` credits the binary, and `vp run <task>` calls the
  script. The commands of the `staged` and `run.tasks` blocks and of the
  `.vite-hooks/` scripts are read like `package.json` scripts and hooks.
  A `vi.mock` call through `vite-plus/test` now counts as a Vitest mock.
