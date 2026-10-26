### Fixed

- **`fallow dead-code` no longer reports the default export of a local Oxlint `jsPlugins` file.** Oxlint loads the default export of each local plugin file. With `includeEntryExports`, fallow reported this export as unused. Fallow now credits the default export, as it does for the Oxlint config file. Other exports of the plugin file stay reportable. Thanks to @Smrtnyk for the report.
  (Closes [#3871](https://github.com/fallow-rs/fallow/issues/3871))
