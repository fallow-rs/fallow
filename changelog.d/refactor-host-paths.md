### Changed

- **An empty `HOME`, `XDG_CONFIG_HOME` or `PATH` now counts as unset in every command.**
  Before, some commands used an empty `HOME` as a relative folder, for example
  a hook install with `--user` and the telemetry config on macOS. They now fall
  back to `USERPROFILE`, as `fallow license` already did, or report that no home
  folder is set. The same `USERPROFILE` fallback now applies to the store of
  `fallow type-aware setup`. An empty `PATH` no longer makes a tool lookup search
  the working directory. Paths for a normal environment do not change.
- **`fallow agent install`, `agent uninstall`, `agent status` and `setup-hooks --user` use `USERPROFILE` when `HOME` is not set.**
  Before, these commands found no home folder in that case. On native Windows,
  where `HOME` is usually not set, they now find the user-level harness files
  under `%USERPROFILE%`.
- **A `HOME` value that is not valid UTF-8 now resolves.** The license path,
  the credentials file and the runtime coverage add-on path use it. Before,
  these paths treated such a `HOME` as unset.
