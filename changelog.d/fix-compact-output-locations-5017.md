### Fixed

- **Compact output for dependency and duplicate-export findings now includes file paths and line numbers.** Lines for `unused-dep`, `unused-devdep`, `unused-optionaldep`, `unlisted-dep`, `duplicate-export`, `type-only-dep`, `test-only-dep`, and `dev-dep-in-prod` now include `<path>:<line>:`, and findings with multiple locations emit one line per location, restoring the uniform `<tag>:<path>:<line>:<subject>` compact grammar.
  (Closes [#5017](https://github.com/fallow-rs/fallow/issues/5017))
