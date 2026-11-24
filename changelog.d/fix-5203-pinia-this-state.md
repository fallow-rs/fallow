### Fixed

- **`unused-store-member` no longer reports option-store state keys that a getter or an action reads through `this.$state`.** A read such as `this.$state.key`, `this.$state['key']` or `const { key } = this.$state` credits that key. When `this.$state` goes to a call or a spread as a whole value, Fallow treats every state key of the store as used. Getters and actions stay checked.
  (Closes [#5203](https://github.com/fallow-rs/fallow/issues/5203))
