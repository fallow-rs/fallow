### Fixed

- **`fallow dead-code` no longer reports unlisted dependencies below a nested `package.json` that declares no dependencies.**
  A nested manifest with only metadata, for example a tegg module with
  `{ "name": "x", "eggModule": {} }`, now inherits the dependency
  declarations of its ancestor manifests. Version 3.34.1 used such a manifest
  as the only owner of the files below it.
