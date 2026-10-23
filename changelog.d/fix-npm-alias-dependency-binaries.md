### Fixed

- **A script binary now credits the npm alias that installs it.** A
  dependency such as `"@typescript/native": "npm:typescript@~7.0.2"` provides
  the binaries of `typescript`, so a `tsc` script no longer reports the alias
  as unused. When the same manifest also declares the aliased package under
  its own name, that declaration provides the binary, and an alias that
  nothing else uses is still reported. A plain declaration in another
  workspace does not change the result. Thanks @waldoluisribeiro for the
  report. (Closes [#3844](https://github.com/fallow-rs/fallow/issues/3844))
