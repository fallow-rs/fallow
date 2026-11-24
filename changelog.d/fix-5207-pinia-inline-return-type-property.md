### Fixed

- **`unused-store-member` no longer reports a store member that code reads through a property typed with an inline `ReturnType<typeof useFooStore>`.** For example, `(deps: { store: ReturnType<typeof useFooStore> }) => deps.store.key` now credits `key`. Nested, destructured and nullable properties (`ReturnType<typeof useFooStore> | null`) work the same way.
  (Closes [#5207](https://github.com/fallow-rs/fallow/issues/5207))
