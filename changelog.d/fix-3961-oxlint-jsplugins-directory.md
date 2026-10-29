### Fixed

- **`fallow dead-code` no longer reports the default export of a local Oxlint `jsPlugins` entry that names a directory or a file without its extension.** Fallow credited the default export only when the entry named the plugin file, for example `"./plugin/index.js"`. Fallow now resolves the entry to the file that Oxlint loads. A directory resolves through the `main` field of its `package.json`, then through its `index` file. A path without an extension resolves to the file with a `.js`, `.mjs`, `.cjs`, `.ts`, `.mts` or `.cts` extension. Other exports of the plugin file stay reportable. Thanks to @Smrtnyk for the report.
  (Closes [#3961](https://github.com/fallow-rs/fallow/issues/3961))
