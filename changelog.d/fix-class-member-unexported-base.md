### Fixed

- **`fallow dead-code` credits class members through an inherited method result in two more shapes.** A subclass can read the result of an inherited `this.find()` when the base class is a class that the file does not export, at any depth of the `extends` chain. A local of a call with arguments, such as `const table = this.findBy(1); table?.reset();`, now credits `reset` on the return type, as the direct `this.findBy(1)?.reset()` does. Before, both shapes reported the members of the return type as unused class members. The extraction cache version changes, so the first run after the upgrade parses each file again.
  (Closes [#4199](https://github.com/fallow-rs/fallow/issues/4199))
