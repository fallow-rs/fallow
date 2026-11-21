### Fixed

- **`effect.schemaExportsAsTypes` now accepts a schema that a function call builds.** With the option on, `export const User = makeUser()` was an `unused-export` finding, where `const makeUser = () => Schema.Struct({...})`. fallow now reads the `return` expressions of the function. The export is a schema when each one is a schema. The check reads syntax only.

  - The function can be in the same module or in an imported module.
  - It must be a function declaration, or a `const` with an arrow function or a function expression.
  - Each path of a block body must end in a `return` or a `throw`.
  - A `return` can use a `const` schema from the top of the function body, and can go through `.pipe(...)`.
  - A `return` can call a second such function, to a depth of 4 functions.
  - An `async` function, a generator, and a function that returns one of its parameters do not count.

  The export of the function itself stays an `unused-export` finding when no module uses it. A `let` or `var` binding that the module assigns again is no longer a schema, because its value is unknown. A run without the option keeps its output. The extraction cache version moves, so the first run after the update parses each file again.
  (Closes [#4988](https://github.com/fallow-rs/fallow/issues/4988))
