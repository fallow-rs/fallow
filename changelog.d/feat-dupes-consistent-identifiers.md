### Added

- **`duplicates.normalization.consistentIdentifiers` finds renamed copies without false matches.** Use it together with `ignoreIdentifiers`. A clone then needs a one-to-one mapping between the names of its instances: `a = b + c` matches `x = y + z`, but not `x = x + x`. When one token breaks the mapping, fallow splits the clone at that token and keeps each part of `minTokens` or more. The option is off by default, and no `mode` turns it on. `fallow migrate` now maps the `ignoreIdentifiers` and `ignoreLiterals` options of an imported duplication config. (Closes [#4626](https://github.com/fallow-rs/fallow/issues/4626))

### Changed

- **`fallow dupes` now compares the static text of template literals.** Before, every template literal matched every other template literal. Now the text outside the `${...}` expressions must match, as for string literals. Thus code that differs only in template text is no longer a clone, and default results can show fewer clone groups. Set `normalization.ignoreStringValues: true`, or use `mode: "weak"` or `"semantic"`, to match template literals with different text again. ([#4626](https://github.com/fallow-rs/fallow/issues/4626))
