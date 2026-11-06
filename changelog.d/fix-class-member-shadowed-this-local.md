### Fixed

- **A shadowing binding no longer credits class members through a `this` method result local.** In a class method, `const table = this.find()` credits the members that the method reads through `table`. A callback parameter, a loop head, a `catch` parameter or a block declaration with the same name holds another value. A read through that binding now gives no credit to the return type of `find()`. A shadow that an `instanceof` guard, an element type or a same-file factory binds to a class credits that class. After the nested scope ends, the outer local gives credit again. This applies to a method of the class itself and to a method of a base class.
  (Closes [#4198](https://github.com/fallow-rs/fallow/issues/4198))
