### Fixed

- **Compact dependency and duplicate-export lines name a path and a line.** The lines of `unused-dep`, `unused-devdep`, `unused-optionaldep`, `type-only-dep`, `test-only-dep` and `dev-dep-in-prod` now have the form `<tag>:<path>:<line>:<package>`, where the path is the `package.json` that declares the package. `unlisted-dep` and `duplicate-export` write one line for each import site or export location. A script that splits a line on `:` now finds the name in field 4, not in field 2.
  (Closes [#5017](https://github.com/fallow-rs/fallow/issues/5017))
