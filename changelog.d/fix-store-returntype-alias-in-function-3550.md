### Fixed

- **`fallow dead-code` now credits a store member that a helper reads through a store alias declared in a function body.** `unused-store-member` resolved `type X = ReturnType<typeof useXStore>` only at the top level of a module or through an import. An alias in a function body or a block now credits the store member for a parameter in the same scope. An inner type of the same name for another type still shadows the alias. The extraction cache version is 386. (Closes [#3550](https://github.com/fallow-rs/fallow/issues/3550))
