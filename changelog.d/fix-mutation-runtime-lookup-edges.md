### Fixed

- **The mutation runtime lookup skips a file that cannot run.** On Unix, a file without an execute bit early on `PATH` no longer hides a real `node` or `bun` later on `PATH`. Before, the run stopped with "Permission denied". This applies to `--runtime node` and to the default runtime lookup.
