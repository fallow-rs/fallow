### Fixed

- **`publicPackages` no longer hides dead code that the public surface of a package cannot reach.** Fallow keeps the entry points of a public package and each declaration that they export or re-export as used. A class or an enum on that surface keeps all of its members. Fallow now reports the other unused exports, types, class members and enum members in the package. Before, fallow removed every such finding in a public package. Unused-file findings do not change.
  (Closes [#4849](https://github.com/fallow-rs/fallow/issues/4849))
