### Fixed

- **`fallow fix` removes the `export` keyword of the right declaration when two declarations share a line.** For `export function live() {} export function dead() {}` with `dead` unused, the fix removed the `export` of `live`. It now removes the `export` of `dead`.
- **`fallow fix` no longer breaks an anonymous default export.** `export default function () {}` and `export default class {}` became `function () {}` and `class {}`, which do not parse. The fix now keeps these exports, and the finding stays in the report.
- **`fallow fix` keeps a shared variable export when a comment follows `export`.** For `export /* values */ const live = 1, dead = 2;` with `dead` unused, the fix removed the `export` of both names. It now keeps the declaration and reports `dead` with `skip_reason: "shared_declaration_in_use"`.
- **`fallow fix` removes the whole unused enum when a member line has a `}` in a comment or a string.** The fix counted each brace character and deleted only the first lines of the enum.
- **`fallow fix` no longer breaks a namespace re-export.** `export * as ns from './x'` became `* as ns from './x'`, which does not parse. The fix now keeps the statement.

### Changed

- **`fallow fix` removes the `export` keyword of a declaration only in a file that parses without an error.** A file with a syntax error, an MDX file and a Glimmer file keep the keyword, and the finding stays in the report. Export lists such as `export { a, b }` are not affected.
