### Fixed

- **`fallow dead-code` now credits Expo config plugins that come from a local array binding.** An app config can hold its `plugins` list in a top-level `const` or `let`, such as `const plugins = [...]; export default { plugins }`. Fallow now reads that list, its spreads of array literals or other stable bindings, and entries or tuples that a binding holds. A binding that the config reassigns, or a dynamic value, still credits no plugin. (Closes [#3883](https://github.com/fallow-rs/fallow/issues/3883))
