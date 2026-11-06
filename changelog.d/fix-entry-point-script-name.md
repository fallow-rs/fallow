### Added

- **`fallow list --entry-points` and `--trace-file` name the package.json script of a script entry point.**
  The human output shows the script key, for example
  ``scripts/build.js (package.json script `build`)`` and
  ``(entry point, package.json script `build`)``. The JSON output adds the
  optional field `script` to each `entry_points[]` item and the optional field
  `entry_script` to the file trace. The `source` string stays
  `package.json script`. When two scripts run the same file, the output names
  the script with the first key in alphabetical order.
  (Closes [#4175](https://github.com/fallow-rs/fallow/issues/4175))
