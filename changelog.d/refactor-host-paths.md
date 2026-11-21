### Changed

- **An empty `HOME`, `XDG_CONFIG_HOME` or `PATH` now counts as unset in every command.**
  Before, some commands used an empty `HOME` as a relative folder, for example
  a hook install with `--user` and the telemetry config on macOS. They now fall
  back to `USERPROFILE`, as `fallow license` already did, or report that no home
  folder is set. The same `USERPROFILE` fallback now applies to the store of
  `fallow type-aware setup`. An empty `PATH` no longer makes a tool lookup search
  the working directory. Paths for a normal environment do not change.
