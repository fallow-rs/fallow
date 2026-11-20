### Fixed

- **`fallow dead-code` no longer reports the class members of a local that a computed key reads when a sibling function has a local of the same name.** Two functions in one file can each declare a local with the same name that holds another class, for example `const view = new A()` and `const view = new B()`. A computed call such as `view[key as keyof A]()` is a use of the whole object, so each member of the class is used. Fallow now credits the class that the local holds at the read, and no longer reports each member of both classes as unused.
  (Closes [#4824](https://github.com/fallow-rs/fallow/issues/4824))
