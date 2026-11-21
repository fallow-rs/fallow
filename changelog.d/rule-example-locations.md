### Fixed

- **Compact output gives the line of a re-export cycle.** A `re-export-cycle:` line now has the line of the re-export in the first file after the path, as a `circular-dependency:` line does: `re-export-cycle:src/a.ts:4:src/a.ts <-> src/b.ts`. The line is `0` when it is not known. A script that splits a compact `re-export-cycle:` line on `:` must read one more field.
