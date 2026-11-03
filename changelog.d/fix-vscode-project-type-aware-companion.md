### Fixed

- **The VS Code extension uses the project `fallow-type-aware` companion.**
  When the extension runs Fallow from the project `node_modules`, it now uses
  the `fallow-type-aware` package from the same `node_modules` if the two
  versions are equal. A project that pins another Fallow version keeps
  type-aware analysis, and a project companion with the `tsrs` backend now
  works in the editor. Otherwise the extension uses its bundled companion. A
  `FALLOW_TYPE_AWARE_BIN` that you set still wins.
  ([#4069](https://github.com/fallow-rs/fallow/issues/4069))
