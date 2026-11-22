### Fixed

- **`effect.schemaExportsAsTypes` knows seven more schema forms.** With the option on, these exports are schemas when each value that they can have is a schema:
  - a conditional expression whose two branches are schemas (`flag ? Schema.String : Schema.Number`), also as the `return` of a function;
  - a member of a namespace import (`Imported.User.pipe(...)`), and a call of a function through one (`Imported.makeUser()`);
  - a call of a function that returns one of its parameters, when the argument at that position is a schema (`identity(Schema.String)`);
  - a member read or a method call of a local `const` object or array literal (`schemas[0]`, `schemas.User`, `factories.make()`);
  - a destructured export of such a member (`export const { User } = { User: Schema.String }`).

  A literal counts only when no code can change its members: the module does not export the literal, reads only members that it has, and does not assign, delete or pass on the literal, and the literal has no spread and no `this`. A parameter that the function assigns or declares again gives no schema. A read of a schema method without a call (`User.annotations`) is a function, so it is no longer a schema. An unused export of each of these forms moves from `unused-export` to `unused-type`. A value of the same form that is not a schema keeps its `unused-export` finding. The extraction cache version moves, so the first run after the update parses each file again.
  (Closes [#5055](https://github.com/fallow-rs/fallow/issues/5055))
