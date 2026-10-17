### Fixed

- **The Oxlint plugin now reads `.oxlintrc.jsonc` and `oxlint.config.mts`.**
  Oxlint finds these config files on its own, but fallow did not read them.
  Packages in their `jsPlugins` lists, at the top level and in `overrides`,
  showed as unused dependencies, and `oxlint.config.mts` showed as an unused
  file. Fallow now reads every config file name that Oxlint finds. Comments
  and trailing commas in `.oxlintrc.jsonc` parse correctly. Thanks
  [@gwynnnplaine](https://github.com/gwynnnplaine) for the report and the
  analysis. (Closes [#3525](https://github.com/fallow-rs/fallow/issues/3525))
