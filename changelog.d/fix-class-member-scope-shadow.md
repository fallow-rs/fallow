### Fixed

- **A module-scope member read no longer credits a class binding of a function body.** In `function setup() { const Alias = new Foo(); }` with a module-scope `const Alias = E; Alias.One`, the read `Alias.One` also credited `Foo.One`. The read now resolves only through the module-scope binding, so `unused-class-members` reports `Foo.One` and `E.One` stays credited. The order of the read and the function does not change the result. (Closes [#4603](https://github.com/fallow-rs/fallow/issues/4603))
