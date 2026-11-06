### Fixed

- **An ESLint `extends` entry with a subpath now credits its package.** Fallow
  used the full entry, such as `eslint-config-shared/back/typescript`, as the
  package name. The package `eslint-config-shared` was then reported as an
  unused dev dependency. Fallow now applies the ESLint naming rules and drops
  the subpath, also for scoped names and short names such as `airbnb/hooks`.
  When the config package is a workspace package that is linked into
  `node_modules`, the file that the entry loads is now used, and its own
  `extends`, `plugins` and `parser` entries get credit.
- **A workspace `eslint.config.mjs` no longer hides the root `.eslintrc.json`.**
  When a workspace flat config had an `import`, fallow did not read the root
  `.eslintrc.json`, `.eslintrc.yaml` or `.eslintrc.yml`. The packages in its
  `extends` were then reported as unused. A plugin config in a source file now
  does not stop the discovery of the JSON and YAML config files of the same
  plugin.
