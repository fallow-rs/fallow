### Fixed

- **A misnamed config file such as `fallow.jsonc` now gives a note.** Discovery reads only `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml` and `.fallow.toml`. When the project root has none of these but has a near-miss name such as `fallow.json`, `fallow.jsonc`, `fallow.yaml` or `fallow.config.json`, the run now reports a `config-file-name-unrecognized` entry in `workspace_diagnostics` and prints a `Note:` line with the accepted names. The note also appears when discovery loads a config from a parent directory, because fallow still does not read the file in the project root. A run with `--config` reports nothing. The language server now reads the config file names from one shared list.
  (Closes [#5166](https://github.com/fallow-rs/fallow/issues/5166))
