### Fixed

- **An unused overloaded method is one `unused_class_members` entry.** Fallow reported one entry for each overload signature and one for the implementation of the method. It now reports one entry at the line of the implementation. A method with bodyless signatures only, for example an abstract method with overloads, gives one entry at its first signature.
  (Closes [#4735](https://github.com/fallow-rs/fallow/issues/4735))
- **`fallow dead-code` no longer reports the static members of a class that code passes on as a value.** When code gives a class to other code, for example as `run(Adapter)`, as `{ adapter: Adapter }`, or through `const a = Adapter; run(a)`, that code can read each static member through an interface type. Each static member of the class is now used, as for an enum that code passes on. The instance members are still reported, because only an instance can read them. The superclass call of `class Sub extends mixin(Base)` does not count.
  (Closes [#4723](https://github.com/fallow-rs/fallow/issues/4723))
- **A read through `Class.prototype.member` is a use of the instance member.** `WebSocketTransport.prototype.handlesUpgrades` now counts as a use of `handlesUpgrades`. A read through a `const` object in the same file also counts, for example `transports[name].prototype.handlesUpgrades` after `const transports = { websocket: WebSocketTransport }`. A holder object that another file exports is not followed yet.
  (Closes [#4724](https://github.com/fallow-rs/fallow/issues/4724))
- **`unused-component-props` does not check a component that `memo` wraps with a comparator.** The comparator of `React.memo(Component, areEqual)` decides when the component renders again, so it can read a prop that the component body does not read. Fallow does not follow the comparator, so it no longer reports the props of such a component. `memo(Component)` without a comparator is still checked.
  (Closes [#4725](https://github.com/fallow-rs/fallow/issues/4725))
