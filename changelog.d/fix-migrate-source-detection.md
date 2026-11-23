### Fixed

- **Migration keeps entry points from `package.json` when a separate file configures duplication or stylesheet rules.** Each tool takes settings from its own config file first.
- **`migrate --from` detects duplication and stylesheet configs with custom file names.** It recognizes `minTokens` alongside `ignore`, and stylesheet `rules` alongside `extends`.
