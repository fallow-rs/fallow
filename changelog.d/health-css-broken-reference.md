### Fixed

- **`fallow health --css` reports `css-broken-reference`.** The `--css` section now runs the project-wide stylesheet pass. Health then reports an `animation` that names a `@keyframes` rule that no stylesheet defines, and a class reference that matches no class. The pass also adds the other cross-file styling findings, for example unused theme tokens. The `check_health` tool of the MCP server does the same when `css` is true. `fallow explain css-broken-reference` now shows an example project.
  (Closes [#4944](https://github.com/fallow-rs/fallow/issues/4944))
