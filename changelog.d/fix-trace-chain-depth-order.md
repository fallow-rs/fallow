### Fixed

- **`fallow trace <file>:<symbol>` keeps every hop inside `--depth`.** The caller and callee walks now visit each module at its shortest depth. Before, a module that the walk first reached through a longer path hid its own imports, even when a direct import put them inside the depth limit.

- **`fallow trace` gives empty chains for a name that the file does not export.** `callers`, `callees` and `unresolved_callees` are now empty when `symbol_found` is `false`, as the output contract says. Before, a typo listed the callees of the whole file, and a namespace importer counted as a caller.
