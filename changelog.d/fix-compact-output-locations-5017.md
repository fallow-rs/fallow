### Fixed

- **Compact dependency and duplicate-export lines name a path and a line.** The lines of `unused-dep`, `unused-devdep`, `unused-optionaldep`, `type-only-dep`, `test-only-dep` and `dev-dep-in-prod` now have the form `<tag>:<path>:<line>:<package>`, where the path is the `package.json` that declares the package. `unlisted-dep:<path>:<line>:<package>` and `duplicate-export:<path>:<line>:<export>` name an import site or an export location, with one line for each location, so these two rules can write more lines than before. A script that splits a line on `:` now finds the name in field 4, not in field 2. To count the findings of one of these two rules as before, select its lines and use `cut -d: -f4 | sort -u`.
  (Closes [#5017](https://github.com/fallow-rs/fallow/issues/5017))
