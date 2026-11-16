### Fixed

- **A type read through a namespace import of a JS module credits its declaration file.** With `import * as lib from "./lib.mjs"` and the type reference `lib.HelperOpts`, the export `HelperOpts` of the sibling `lib.d.mts` no longer reports as an unused type under `includeEntryExports`. The same applies to a `.js` module with `.d.ts` and a `.cjs` module with `.d.cts`. (Closes [#4551](https://github.com/fallow-rs/fallow/issues/4551))
