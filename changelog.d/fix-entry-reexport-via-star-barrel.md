### Fixed

- **Entry-point named re-exports through `export *` barrels credit the underlying declaration.** `fallow dead-code` and `fallow dead-code --trace` dropped entry-point credit when a configured entry file re-exported a type or value by name from an intermediate `export *` barrel, reporting the source declaration as unused. (Fixes [#4579](https://github.com/fallow-rs/fallow/issues/4579))
