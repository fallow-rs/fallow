### Changed

- **`fallow coverage analyze` reads `production` from the config.** Without `--production`, the command analyzed in non-production mode. It now uses the `production` setting of the config, as `fallow dead-code` does.
- **`fallow security` reads the base config from the base commit.** When `fallow security --gate newly-reachable` compares with a base ref, the base side now uses the config file of that commit, as the second base pass of `fallow audit` does. A config change in the pull request then shows as a change in the findings.

### Fixed

- **`fallow inspect` now applies `--allow-remote-extends` to each part of the bundle.** The child processes that collect the evidence did not get the flag, so a config with an `https://` entry in `extends` failed there. The type-aware evidence of a symbol now also reads the flag and the `FALLOW_PRODUCTION*` variables, as the child processes do.
