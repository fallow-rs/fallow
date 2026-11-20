### Fixed

- **`fallow health --coverage` and runtime coverage find a `webpack://` or `vite://` source with an escaped character in its path.** Node writes a space in such a source as `%20`. Fallow now decodes the path before it looks for the file, so the source gets its coverage.
- **A V8 dump with one malformed `source-map-cache` entry keeps its other evidence.** Fallow reads each entry on its own and skips only the bad one. Before, the bad entry removed every source map of the dump, and `fallow health --coverage` skipped the full dump.
- **A script URL that one V8 dump records two times adds the counts of both records to its original sources.** Before, only the first record used the source map.
- **`fallow health --coverage` resolves a relative source against the directory of an external source map.** Before, it used the directory of the script, so a map in another directory gave no coverage.
