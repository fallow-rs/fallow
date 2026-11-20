### Fixed

- **`fallow migrate` keeps the gate of the source config.** A source config for unused code reports each issue type as an error unless its `rules` say otherwise. Some fallow rules default to `warn`, so a migrated project could pass a run that failed before, for example on an unused `devDependencies` entry. The migrated config now sets `unused-dev-dependencies`, `unused-namespace-members`, `unlisted-binaries` and `duplicate-export-aliases` to `error`, unless the source config sets another severity or turns the issue type off. The migration prints one note that names these rules. `dependencies` in an `include` or `exclude` list of the source config now also covers `devDependencies`. Before, `include: ["files", "dependencies"]` wrote `"unused-dev-dependencies": "off"`. The fallow default severities do not change. A config that an earlier version migrated stays as it is: set these rules to `error` by hand to get the same gate.
  (Closes [#4905](https://github.com/fallow-rs/fallow/issues/4905))

### Changed

- **A dead-code report with findings that are all warnings says why the exit code stays 0.** The human report prints one line under the status line of the dead-code findings: "Warnings do not change the exit code. Set a rule to `error` to fail on it." The line shows in the default, `--summary` and `--group-by` reports, and in the dead-code part of `fallow audit` and of a bare `fallow` run. The machine formats and `--quiet` runs do not change.
  (Refs [#4905](https://github.com/fallow-rs/fallow/issues/4905))
