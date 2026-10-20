### Fixed

- **`fallow dead-code` no longer reports unlisted dependencies below a nested `package.json` without dependency fields.**
  A nested manifest with only metadata, for example a tegg module with
  `{ "name": "x", "eggModule": {} }`, now inherits the dependency
  declarations of its ancestor manifests. Version 3.34.1 used such a manifest
  as the only owner of the files below it. A manifest with an explicit empty
  field, such as `"dependencies": {}`, stays the owner of its files.
