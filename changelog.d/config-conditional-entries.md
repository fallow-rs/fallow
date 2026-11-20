### Fixed

- **An entry list that a config picks with a conditional expression now gives its entry points.** A config can pick its entries from the environment, as in `entry: isBootstrap ? ["src/bin.ts"] : ["src/bin.ts", "src/worker.ts"]`. Fallow read no entry from such a value, so it reported the files as unused. It now reads the files of both branches, for each plugin that reads a path or a list of paths from a config, such as the `pack.entry` of a Vite+ config and the `build.lib.entry` of a Vite config.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))
