### Fixed

- **The CLI reference names `fallow architecture` for the architecture issue types.** `fallow schema --cli-reference`, `fallow schema` and the issue registry gave the cycle, boundary and policy issue types the command `dead-code` and a deprecated option, for example `--circular-deps`. Now these seven rows give `command: "architecture"` and `--cycles`, `--boundaries` or `--policy`. The new `deprecated_filter_flag` field keeps the old option. `fallow dead-code --circular-deps` and the other old options work as before, and MCP `issue_types` still selects these findings. (Closes [#4883](https://github.com/fallow-rs/fallow/issues/4883))

### Added

- **A `deprecated` field for deprecated commands and options.** `fallow schema` and `fallow schema --cli-reference` give each deprecated command and option a `deprecated` object with the `replacement`. The field is absent on a current command or option.

### Deprecated

- **The `fallow dead-code` options for the architecture issue types print a warning.** `--circular-deps`, `--re-export-cycles`, `--package-cycles`, `--boundary-violations` and `--policy-violations` print one stderr warning each, with the `fallow architecture` option to use. `--quiet` removes the warning. The next major version removes these options.
