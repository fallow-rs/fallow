### Fixed

- **CLI JSON errors include recovery guidance for invalid arguments, invalid roots and missing fix confirmation.** Stable codes identify each failure, and `help` describes the next step. Existing exit statuses are preserved.
- **Skipped fixes explain how to obtain a fresh preview.** JSON skip records include `help` for changed files, mixed line endings and incomplete analysis while preserving the safety skips.
- **Invalid JSON presentation options return structured JSON errors.** Unsupported `--lean` combinations preserve the selected compact or pretty output style.
