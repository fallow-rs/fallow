### Fixed

- **`fallow dead-code` uses the element class of a `for...of` loop variable only in the loop body.** After `for (const t of tables)` with `tables: Table[]`, a parameter `t` of another type in another function no longer credits `Table` members. This applies also to a loop over a call, such as `for (const t of all())`.
  (Closes [#4352](https://github.com/fallow-rs/fallow/issues/4352))
- **`fallow dead-code` no longer credits a member through `this` in a function with its own `this`.** In a nested `function` or an object-literal method inside a class method, `this.find().setVip()` no longer credits `setVip` on the return type of `find()`. An arrow function keeps the `this` of the method and still gives the credit. A bare `this.find()` read in such a function keeps the file-level credit that every function with its own `this` gets, because code often binds the instance with `.bind(this)` or `.call(this)`. The extraction cache version is 423.
  (Closes [#4351](https://github.com/fallow-rs/fallow/issues/4351))
