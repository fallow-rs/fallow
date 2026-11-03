### Fixed

- **`effect.schemaExportsAsTypes` now covers re-exported and derived schemas.** A module that exports a schema of another module again (`import { User } from "./schemas"; export { User }`, or `export { User } from "./schemas"`) now reports it as an unused type, also through `export *` barrels. A chain on an imported schema, such as `export const UserId = User.pipe(Schema.brand("UserId"))`, also counts as a schema. A re-export of a value that is not a schema stays an unused export.
