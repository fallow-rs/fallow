### Added

- **`fallow schema --cli-reference` prints a full CLI reference as one JSON document.**
  The document lists every command and subcommand at each depth, in the
  order and the groups of `fallow --help`, with its usage line, its full help
  and its options. Each option has its value name, aliases, possible values,
  default, help heading, conflicts and the `--x`/`--no-x` pair. Hidden
  commands and options stay in the document with `hidden: true`. The document
  also has the exit codes with a name for each code, the exit codes whose
  meaning a command defines itself, the environment variables with a group
  and a default, the output formats, the MCP tools and resources, and each
  issue type with a sentence-case `display_label` and the text of
  `fallow explain <id> --format json`. A documentation generator can write
  the command pages, the option tables and the issue type pages from this
  one file. The repository commits the output as `docs/cli-reference.json`.
  In the default `fallow schema` output, the row of the `schema` command
  gains the `--cli-reference` flag.
- **`fallow dupes --help` groups its options under Detection, Output and Gates.**
  The flags of the `dupes` row in the default `fallow schema` output follow
  the same order, so `--threshold` now comes after `--trace`.

### Fixed

- **`fallow schema` lists the environment variables that it did not list.**
  The `environment_variables` map now has `FALLOW_CLOCK_EPOCH`,
  `FALLOW_MCP_MAX_OUTPUT_BYTES` and the four `FALLOW_GATE_*` variables of the
  agent gate script.
- **`fallow schema` names the rules key of the complexity and file-length findings.**
  The `high-cyclomatic-complexity`, `high-cognitive-complexity`,
  `high-crap-score` and `long-file` rows had a null `config_key`,
  `default_severity` and `opt_in`, although `rules.complexity-cyclomatic`,
  `rules.complexity-cognitive`, `rules.complexity-crap` and
  `rules.complexity-file-lines` decide if these findings fail a run. The rows
  now name these keys, their default `error` and `opt_in: false`. The
  `fallow://issue-types` MCP resource and `npm/fallow/issue-registry.json`
  give the same values.
