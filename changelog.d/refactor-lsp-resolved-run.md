### Fixed

- **The editor keeps the `fallow.production` setting when the project config does not load.** The language server then analyzes with the default config, as before, but it now also keeps the production setting and the duplication settings of the editor. An invalid external plugin file in the config now also gives this fallback and a warning.
- **The editor does no duplication work when `duplicates.enabled` is `false`.** The diagnostics stay the same. Before, the language server ran the duplication analysis and then dropped its result.
