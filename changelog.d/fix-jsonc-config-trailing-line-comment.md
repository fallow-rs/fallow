### Fixed

- **A JSON or JSONC config file that ends with a `//` line comment and no
  final newline is now read.** Before this fix, the comment hid the end of
  the config, so the parse failed and fallow read nothing from the file.
  This affected `.oxlintrc.jsonc` and the JSON configs of the ESLint,
  Prettier, TypeScript, Jest and Babel plugins. For example, a
  `tsconfig.json` with a final comment lost its `extends` package, and an
  `.oxlintrc.jsonc` lost its `jsPlugins` packages, which were then reported
  as unused dependencies.
