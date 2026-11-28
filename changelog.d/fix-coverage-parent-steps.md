### Fixed

- **Coverage paths with `..` match the same source file on each machine.** fallow applied a `..` in a recorded coverage path only when the directory before it existed, so a file could get its coverage on one machine and a static estimate on another. fallow now applies each `..` to the recorded text first. This covers Istanbul paths, V8 script and source map paths, and the search for a `webpack://` or `vite://` source.
