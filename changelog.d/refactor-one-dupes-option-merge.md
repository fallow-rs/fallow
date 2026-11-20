### Changed

- **The language server warns about a `duplication` or `typeAware` initialization option that does not parse.** Before, the server ran without the option and said nothing. It now writes a warning that names the option to the log of the editor, for example when `duplication.near` is a string.
