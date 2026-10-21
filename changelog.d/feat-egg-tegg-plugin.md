### Added

- **Fallow now detects Egg.js and tegg projects.** The new `egg` plugin
  activates on `egg`, `@eggjs/core` or `@eggjs/tegg`, or on an `egg`,
  `eggPlugin` or `eggModule` key in `package.json`. It marks the files that
  the Egg loader loads by convention as entry points: `app.js`, `agent.js`,
  the router, controllers, services, middleware, extensions, schedules and
  config files, also in nested example and test fixture apps. It marks every
  file of a tegg module as an entry point. It credits the packages that
  `config/plugin.*` names with `package`, the `egg.framework` package and the
  default `egg` framework to the `package.json` that owns the app.
