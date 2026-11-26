### Fixed

- **`unused-class-member` credits members that a MobX annotation object names.** A static key in the annotation object of `makeObservable(this, {...})` or `makeAutoObservable(this, {...})` from `mobx` now counts as a use of the member with that name. MobX reads these members when the constructor runs.
  (Closes [#5297](https://github.com/fallow-rs/fallow/issues/5297))
- **`unused-class-member` credits four more receiver forms whose class the syntax proves.** These forms are now credited: a factory whose every return gives `new X()` inside a branch and whose body ends in `throw`, a factory that returns a subclass without own members, a static method that returns `new X()` through methods that return `this`, and a destructured property of an object factory result (`const { ctx } = newService(); ctx.member()`).
  (Closes [#5298](https://github.com/fallow-rs/fallow/issues/5298))
- **`unused-class-member` keeps the members that an implemented interface inherits.** A class that implements an interface now keeps each required member of each interface that the implemented interface extends, also when the parent interface is in another module.
  (Closes [#5299](https://github.com/fallow-rs/fallow/issues/5299))
