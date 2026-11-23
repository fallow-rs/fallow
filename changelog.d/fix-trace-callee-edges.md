### Fixed

- **`fallow trace --callees` stops at side-effect-only imports.** A trace of a file that imports `./setup` for its side effects used to include the imports of `./setup` as disconnected hops.
- **`fallow trace --callees` reports calls to external imports.** For example, a call to `parse` from the `yaml` package appears in `unresolved_callees` when the graph has no edge to the package.
