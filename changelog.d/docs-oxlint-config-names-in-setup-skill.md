### Fixed

- **The `fallow-setup` skill now detects all Oxlint and Oxfmt config file
  names.** The tooling detection table listed only `.oxlintrc.json` and
  `.oxfmtrc.json`. An agent that followed the skill could miss an existing
  linter or formatter and propose a second one. The table now also lists
  `.oxlintrc.jsonc`, `oxlint.config.{ts,mts}`, `.oxfmtrc.jsonc` and
  `oxfmt.config.*`.
