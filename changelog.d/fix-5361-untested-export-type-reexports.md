### Fixed

- **`untested-export` no longer reports a re-export of a type as an untested runtime export.** A re-export without the `type` keyword, for example `export { PathValue } from './eager'` where `eager.ts` declares `export type PathValue = ...`, was a value export in `fallow health --coverage-gaps`. TypeScript erases such a re-export, so no test can run it. The rule now follows the re-export chain. It skips the export when the name resolves to a type alias or an interface and to no value. A class, an enum, or a `const` with a type of the same name stays a runtime export. A file that only re-exports types in this way is no longer a runtime file, and the `value_export_count` of an untested file uses the same check.
  (Closes [#5361](https://github.com/fallow-rs/fallow/issues/5361))
