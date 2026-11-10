### Changed

- **The function contract facts in the extraction cache now mark three more open shapes.** A direct `eval` anywhere in a module is recorded for the module, `-0` is a value different from `0`, and a read of `arguments` in a class decorator, `extends` clause or computed key inside a function opens the contract of that function. No report changes. The first run after the upgrade parses each file again, because the cache format changed.
  (Part of [#4491](https://github.com/fallow-rs/fallow/issues/4491))
