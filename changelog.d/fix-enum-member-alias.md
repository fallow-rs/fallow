### Fixed

- **`fallow dead-code` no longer reports enum members that code reads through an alias, or of an enum that code passes on whole.**
  A member read through `const Alias = Color`, through a class field such as
  `protected readonly levels = Level`, through `this.levels.High`, or through
  an Angular template (inline or `templateUrl`) that reads the field is now
  used. Members that no alias reads are still reported. When code passes the
  whole enum on, for example as `takesEnum(Shape)` or `{ ref: Size }`, each
  member of that enum is now used, as with `Object.values(Shape)`. A
  `typeof E` (for example `keyof typeof E`) now counts as a use of every
  member of `E`.
  (Closes [#4578](https://github.com/fallow-rs/fallow/issues/4578))
