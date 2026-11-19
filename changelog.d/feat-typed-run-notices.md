### Added

- **Run notices are part of the JSON output.** When a flag has no effect, when the config does not load in the editor, when two config files exist in one directory, when `security.categories` names an unknown category, or when a `FALLOW_PRODUCTION*` variable has a value that fallow cannot read, the JSON output now has a `workspace_diagnostics` entry for it. The new kinds are `inert-flag`, `config-load-fell-back`, `config-file-shadowed`, `unknown-security-category` and `env-value-ignored`. An entry names an environment variable and never holds its value. `fallow audit --format json` has a root `workspace_diagnostics` array when such a notice exists. MCP tools show these notices in `warnings`, and the degraded-run warning now counts the diagnostics of every section of an audit. The language server writes them to its log. The human output prints a flag without effect and an ignored variable as one `Note:` line, and `--quiet` hides it.

### Changed

- **The `--dupes-baseline` note follows `--quiet` and the output format.** When `duplicates.enabled: false` turns the duplication analysis off, the note now prints only in the human output and without `--quiet`. `--production` on `fallow trace --path`, a re-export chain trace and `fallow trace-error` now prints a note, because these commands read the production mode from the config only.
