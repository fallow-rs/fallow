### Fixed

- **The editor quick fix "Remove unused export" no longer breaks an anonymous default export.** For `export default function () {}`, `export default class {}` and `export default { ... }`, the quick fix removed `export default `, and the code did not parse or changed its meaning. The quick fix is no longer offered for these exports.
- **The editor quick fix "Remove unused export" keeps the export of a name that is in use.** For `export const dead = 1, live = 2;` with only `dead` unused, the quick fix removed the `export` of both names. It is now offered only when each name of the declaration is unused.

### Changed

- **The editor quick fix "Remove unused export" is not offered in a document with a syntax error.** The quick fix reads the declarations from the parsed document text. The finding stays in the report.
- **The editor quick fix "Remove unused export" is offered for the second declaration on a line.** For `export function live() {} export function dead() {}` with `dead` unused, the quick fix removes the `export` of `dead`. Before, it offered no quick fix.
