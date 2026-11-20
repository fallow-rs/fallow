### Added

- **`fallow config --resolved` shows the type-aware setting.** The new `settings.type_aware` member gives the on or off value and its source: a flag or an option, `FALLOW_TYPE_AWARE`, `audit.typeAware`, or `typeAware.enabled`. It is `null` when `FALLOW_TYPE_AWARE` or `FALLOW_TYPE_AWARE_REQUIRE` has a value that the run cannot use.

### Fixed

- **`fallow health` rejects a `FALLOW_TYPE_AWARE` value that is not valid UTF-8.** `fallow dead-code` already stopped with exit code 2 for such a value, but `fallow health` ignored the variable. Both commands now give the same error. A `FALLOW_TYPE_AWARE_REQUIRE` value that is not valid UTF-8 now also stops each command that reads type-aware settings with exit code 2. Before, the commands ignored it.
