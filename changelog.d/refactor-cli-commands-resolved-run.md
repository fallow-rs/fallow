### Changed

- **`fallow security` reads the base config from the base commit.** When `fallow security --gate newly-reachable` compares with a base ref, the base side now uses the config file of that commit, as the second base pass of `fallow audit` does. A config change in the pull request then shows as a change in the findings.
