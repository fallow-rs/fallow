### Fixed

- **`unused-store-member` no longer reports Pinia state keys that code reads through `store.$state`.** A read such as `store.$state.key`, `store.$state['key']` or `const { key } = store.$state` credits that key. When the `$state` object goes to a call, a spread or a local, Fallow treats the whole store as used. A whole write `store.$state = { ... }` reads no key.
  (Closes [#5168](https://github.com/fallow-rs/fallow/issues/5168))
