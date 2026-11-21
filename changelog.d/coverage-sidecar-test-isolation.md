### Fixed

- **The coverage unit tests no longer fail on a machine that has the runtime coverage add-on.** Sidecar discovery now takes its home folder, `PATH` and environment overrides as one input value. The `fallow coverage setup` flow and the tests give that value to the discovery, so an add-on in `~/.fallow/bin` or on `PATH` of the host does not change a test result.
  (Closes [#4935](https://github.com/fallow-rs/fallow/issues/4935), closes [#4933](https://github.com/fallow-rs/fallow/issues/4933))
