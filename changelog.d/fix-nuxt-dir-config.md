### Fixed

- **The Nuxt plugin reads the `dir` object of `nuxt.config`.** A string value of
  `dir.pages`, `dir.layouts`, `dir.middleware` or `dir.plugins` adds the
  convention entry points of that directory, relative to `srcDir`. A string
  value of `dir.modules` adds them relative to the project root, as Nuxt does.
  Files in a custom directory are no longer reported as unused.
  (Closes [#4176](https://github.com/fallow-rs/fallow/issues/4176))
