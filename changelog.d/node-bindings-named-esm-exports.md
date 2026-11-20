### Fixed

- **Named ES module imports of `@fallow-cli/fallow-node` now work.** `import { detectDeadCode } from "@fallow-cli/fallow-node"` failed with `SyntaxError: Named export 'detectDeadCode' not found`. The package now has an ES module entry that exports each binding function by name. The default export and the CommonJS `require` entry do not change.
  (Closes [#4840](https://github.com/fallow-rs/fallow/issues/4840))
