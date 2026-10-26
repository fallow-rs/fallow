### Fixed

- **Config values in a same-file constant are now read.** A config can keep a
  part of its value in a top-level constant, for example
  `const UNIT = { test: { setupFiles: [...] } }` with `projects: [UNIT]`, or
  `setupFiles: SETUP` and `test: TEST`. Fallow read only inline values, so it
  reported the setup files of such a Vitest project as unused files. The config
  parser now uses the value of the constant in each property value and array
  element that names it. This applies to all plugins that read config files. A
  constant that the file declares more than one time or writes to stays
  unresolved. Thanks [@Smrtnyk](https://github.com/Smrtnyk) for the report.
  (Closes [#3852](https://github.com/fallow-rs/fallow/issues/3852))
