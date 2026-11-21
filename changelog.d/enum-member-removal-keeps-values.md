### Fixed

- **`fallow fix` no longer changes the value of a kept enum member.** A member without an initializer counts on from the member before it. When `fix` removes an unused member, the next kept member now gets an initializer with its old value, so `Low, Medium, High` with `Low` unused becomes `Medium = 1, High`. Each later member keeps its value too. When the old value is not a known integer, for example after a computed initializer or in a `declare enum`, `fix` keeps the enum as it is. The fix JSON does not change.
  (Closes [#4963](https://github.com/fallow-rs/fallow/issues/4963))
