### Fixed

- **The `jsPlugins` of an imported Oxlint preset are no longer unused devDependencies.** An Oxlint config can extend a preset that it imports as a value, for example `extends: [reactStrict]`. fallow now resolves the import, also under `node_modules`, and credits the `jsPlugins` of its default export when that export is a static object. fallow also follows the imported presets of that preset, to a depth limit. A preset that fallow cannot read statically gives no credit and no error.
  (Closes [#5101](https://github.com/fallow-rs/fallow/issues/5101))
