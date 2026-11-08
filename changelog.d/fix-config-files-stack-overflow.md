### Fixed

- **A crafted TOML file no longer crashes fallow.** A `fallow.toml`, an external plugin file, or a TOML file that a framework plugin reads (`bunfig.toml`, `mise.toml`, `wrangler.toml`, `.prettierrc.toml`, GraphQL config) could crash any command with a stack overflow. Fallow now rejects TOML that nests arrays and inline tables deeper than 128 levels before it parses the file. A rejected config file gives a parse error. A rejected plugin file is skipped, as with other invalid TOML.
