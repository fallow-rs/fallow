### Changed

- **The built-in ignore list now has the root `fallow-viz.html`.** `fallow viz` writes its map to `fallow-viz.html` in the project root. Discovery parsed that file as an HTML source file on the next run, and the next map showed the map itself. A file of this name in the project root is now not analyzed. Add `"!fallow-viz.html"` to `ignorePatterns` to analyze it again. The rule does not apply to a file of this name in a subfolder.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))

### Fixed

- **`fallow list --boundaries` shows the `root` of a zone.** The human output prints `(root <folder>)` after the patterns of a zone that has a `root`. The JSON zone object has a new optional `root` field, which is omitted when the zone has no `root`.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
