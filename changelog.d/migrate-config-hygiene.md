### Fixed

- **`fallow migrate` writes each rule severity in the form that fallow reads.** A severity such as `"Warning"` or `"OFF"` becomes `warn` or `off`. A value that is not `error`, `warn` or `off`, or that is not a string, now gives one warning for its source rule, and the rule is not migrated. Before, the config kept the value and did not load, or the rule disappeared with no warning.
- **A line break in a source path stays inside the `Migrated from` comment.** `fallow migrate` writes a `\n` or `\r` in the path as an escape, in TOML and in JSONC output. Before, the rest of the path became a config line, and the written config did not load.
